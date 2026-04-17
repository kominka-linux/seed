use std::io::{Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, stdout};

const APPLET: &str = "tr";

#[derive(Clone, Debug, Default)]
struct Options {
    delete: bool,
    squeeze: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let (options, set1, set2) = parse_args(args)?;
    let set1 = expand_set(set1)?;
    let set2 = expand_set(set2.unwrap_or(""))?;

    let mut map = [None; 256];
    if !options.delete {
        build_translation_map(&set1, &set2, &mut map);
    }

    let squeeze_set: &[u8] = if options.squeeze {
        if !set2.is_empty() { &set2 } else { &set1 }
    } else {
        &[]
    };

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = stdout();
    let mut buf = [0u8; BUFFER_SIZE];
    let mut previous: Option<u8> = None;

    loop {
        let read = input
            .read(&mut buf)
            .map_err(|e| vec![AppletError::from_io(APPLET, "reading stdin", None, e)])?;
        if read == 0 {
            break;
        }

        for &byte in &buf[..read] {
            if options.delete && set1.contains(&byte) {
                continue;
            }

            let translated = map[byte as usize].unwrap_or(byte);
            if options.squeeze && previous == Some(translated) && squeeze_set.contains(&translated)
            {
                continue;
            }

            out.write_all(&[translated])
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            previous = Some(translated);
        }
    }

    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options, &str, Option<&str>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut positionals = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'd' => options.delete = true,
                    's' => options.squeeze = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        positionals.push(arg);
    }

    match positionals.as_slice() {
        [set1] if options.delete || options.squeeze => Ok((options, set1, None)),
        [set1, set2] => Ok((options, set1, Some(set2))),
        [set1] => Err(vec![AppletError::new(
            APPLET,
            format!("missing operand after '{set1}'"),
        )]),
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn expand_set(input: &str) -> Result<Vec<u8>, Vec<AppletError>> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let current = parse_atom(bytes, &mut i)?;
        if i + 1 < bytes.len() && bytes[i] == b'-' {
            let mut lookahead = i + 1;
            let end = parse_atom(bytes, &mut lookahead)?;
            if current <= end {
                for b in current..=end {
                    out.push(b);
                }
            } else {
                for b in (end..=current).rev() {
                    out.push(b);
                }
            }
            i = lookahead;
        } else {
            out.push(current);
        }
    }

    Ok(out)
}

fn parse_atom(bytes: &[u8], index: &mut usize) -> Result<u8, Vec<AppletError>> {
    let Some(&byte) = bytes.get(*index) else {
        return Err(vec![AppletError::new(APPLET, "invalid character set")]);
    };
    *index += 1;

    if byte != b'\\' {
        return Ok(byte);
    }

    let Some(&escaped) = bytes.get(*index) else {
        return Ok(b'\\');
    };
    *index += 1;
    Ok(match escaped {
        b'n' => b'\n',
        b't' => b'\t',
        b'r' => b'\r',
        b'a' => b'\x07',
        b'b' => b'\x08',
        b'f' => b'\x0C',
        b'v' => b'\x0B',
        b'\\' => b'\\',
        d @ b'0'..=b'7' => {
            // Octal escape: up to 3 digits total (the first was already consumed as `d`).
            let mut val = (d - b'0') as u32;
            for _ in 0..2 {
                match bytes.get(*index) {
                    Some(&d2 @ b'0'..=b'7') => {
                        val = val * 8 + (d2 - b'0') as u32;
                        *index += 1;
                    }
                    _ => break,
                }
            }
            val as u8
        }
        other => other,
    })
}

fn build_translation_map(set1: &[u8], set2: &[u8], map: &mut [Option<u8>; 256]) {
    if set1.is_empty() || set2.is_empty() {
        return;
    }

    let last = *set2.last().expect("non-empty set2");
    for (idx, &src) in set1.iter().enumerate() {
        let dest = set2.get(idx).copied().unwrap_or(last);
        map[src as usize] = Some(dest);
    }
}

#[cfg(test)]
mod tests {
    use super::{build_translation_map, expand_set, parse_args};
    use std::ffi::OsString;

    #[test]
    fn parses_delete_and_squeeze_flags() {
        let args = vec![OsString::from("-ds"), OsString::from("a-z")];
        let (options, set1, set2) = parse_args(&args).unwrap();
        assert!(options.delete);
        assert!(options.squeeze);
        assert_eq!(set1, "a-z");
        assert_eq!(set2, None);
    }

    #[test]
    fn expands_ranges_and_escapes() {
        assert_eq!(expand_set("a-c").unwrap(), b"abc");
        assert_eq!(expand_set("\\n\\t").unwrap(), b"\n\t");
    }

    #[test]
    fn translation_uses_last_destination_byte_for_padding() {
        let mut map = [None; 256];
        build_translation_map(b"abcd", b"XY", &mut map);
        assert_eq!(map[b'a' as usize], Some(b'X'));
        assert_eq!(map[b'b' as usize], Some(b'Y'));
        assert_eq!(map[b'd' as usize], Some(b'Y'));
    }

    #[test]
    fn translation_requires_two_sets_without_delete_or_squeeze_only_mode() {
        let args = vec![OsString::from("abc")];
        assert!(parse_args(&args).is_err());
    }
}
