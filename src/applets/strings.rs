use std::io::Read;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "strings";
const DEFAULT_MIN_LEN: usize = 4;

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut min_len = DEFAULT_MIN_LEN;
    let mut paths: Vec<&str> = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-n" | "--bytes" => {
                let val = cursor.next_value(APPLET, "-n")?;
                min_len = val.parse().map_err(|_| {
                    vec![AppletError::new(
                        APPLET,
                        format!("invalid minimum string length '{val}'"),
                    )]
                })?;
            }
            a if a.starts_with("--bytes=") => {
                let val = &a["--bytes=".len()..];
                min_len = val.parse().map_err(|_| {
                    vec![AppletError::new(
                        APPLET,
                        format!("invalid minimum string length '{val}'"),
                    )]
                })?;
            }
            a if a.starts_with("-n") && a.len() > 2 => {
                let val = &a[2..];
                min_len = val.parse().map_err(|_| {
                    vec![AppletError::new(
                        APPLET,
                        format!("invalid minimum string length '{val}'"),
                    )]
                })?;
            }
            // -a / --all: scan entire file — always our behavior, so accept and ignore
            "-a" | "--all" | "--data" => {}
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap(),
                )]);
            }
            a => paths.push(a),
        }
    }

    let paths: Vec<&str> = if paths.is_empty() { vec!["-"] } else { paths };
    let mut out = stdout();
    let mut errors = Vec::new();

    for &path in &paths {
        match open_input(path) {
            Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
            Ok(mut f) => {
                let mut data = Vec::new();
                if let Err(e) = f.read_to_end(&mut data) {
                    errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                    continue;
                }
                if let Err(e) = extract_strings(&data, min_len, &mut out) {
                    return Err(vec![AppletError::from_io(APPLET, "writing", None, e)]);
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_printable(b: u8) -> bool {
    // ASCII printable: space (0x20) through tilde (0x7e), plus tab (0x09)
    b == b'\t' || (b' '..=b'~').contains(&b)
}

fn extract_strings<W: std::io::Write>(
    data: &[u8],
    min_len: usize,
    out: &mut W,
) -> std::io::Result<()> {
    let mut start = 0;
    let mut in_run = false;

    for (i, &b) in data.iter().enumerate() {
        if is_printable(b) {
            if !in_run {
                start = i;
                in_run = true;
            }
        } else if in_run {
            in_run = false;
            if i - start >= min_len {
                out.write_all(&data[start..i])?;
                out.write_all(b"\n")?;
            }
        }
    }
    // Flush any trailing run
    if in_run && data.len() - start >= min_len {
        out.write_all(&data[start..])?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    fn extract(data: &[u8], min_len: usize) -> String {
        let mut out = Vec::new();
        extract_strings(data, min_len, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn basic_extraction() {
        let data = b"hello\x00world\x00";
        assert_eq!(extract(data, 4), "hello\nworld\n");
    }

    #[test]
    fn min_len_filter() {
        // "hi" is 2 chars, below default minimum of 4
        let data = b"hi\x00long_enough\x00";
        assert_eq!(extract(data, 4), "long_enough\n");
        assert_eq!(extract(data, 2), "hi\nlong_enough\n");
    }

    #[test]
    fn non_printable_splits_runs() {
        let data = b"abc\x01def";
        // Both "abc" and "def" are length 3 — below default min of 4
        assert_eq!(extract(data, 4), "");
        assert_eq!(extract(data, 3), "abc\ndef\n");
    }

    #[test]
    fn trailing_string_without_terminator() {
        let data = b"\x00hello world";
        assert_eq!(extract(data, 4), "hello world\n");
    }

    #[test]
    fn empty_input() {
        assert_eq!(extract(b"", 4), "");
    }

    #[test]
    fn tab_is_printable() {
        let data = b"col1\tcol2\x00";
        assert_eq!(extract(data, 4), "col1\tcol2\n");
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"])).is_err());
    }

    #[test]
    fn nonexistent_file_fails() {
        assert!(run(&args(&["__no_such_file__"])).is_err());
    }

    #[test]
    fn parses_long_and_attached_byte_count() {
        assert!(run(&args(&["--bytes=3"])).is_ok());
        assert!(run(&args(&["-n3"])).is_ok());
    }
}
