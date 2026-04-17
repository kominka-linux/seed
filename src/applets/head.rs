use std::io::{self, BufRead, BufReader, Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, open_input, stdout};

const APPLET: &str = "head";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut lines: Option<i64> = None;
    let mut bytes: Option<i64> = None;
    let mut quiet = false;
    let mut verbose = false;
    let mut paths: Vec<&str> = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-q" | "--quiet" | "--silent" => quiet = true,
            "-v" | "--verbose" => verbose = true,
            "-n" | "--lines" => {
                lines = Some(parse_count(APPLET, cursor.next_value(APPLET, "n")?)?);
            }
            "-c" | "--bytes" => {
                bytes = Some(parse_count(APPLET, cursor.next_value(APPLET, "c")?)?);
            }
            // -NUM shorthand: head -5 ≡ head -n 5
            a if a.starts_with('-')
                && a[1..]
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false) =>
            {
                lines = Some(parse_count(APPLET, &a[1..])?);
            }
            // -n<N> and -c<N> attached forms: head -n1 ≡ head -n 1
            a if a.starts_with("-n") && a.len() > 2 => {
                lines = Some(parse_count(APPLET, &a[2..])?);
            }
            a if a.starts_with("-c") && a.len() > 2 => {
                bytes = Some(parse_count(APPLET, &a[2..])?);
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => paths.push(arg),
        }
    }

    let n = lines.unwrap_or(10);
    let multiple = paths.len() > 1;
    let mut out = stdout();
    let mut errors = Vec::new();

    if paths.is_empty() {
        if verbose {
            writeln!(out, "==> standard input <==")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
        let stdin = io::stdin();
        let result = if let Some(bc) = bytes {
            head_bytes(&mut stdin.lock(), &mut out, bc)
        } else {
            head_lines(&mut BufReader::new(stdin.lock()), &mut out, n)
        };
        if let Err(e) = result {
            return Err(vec![AppletError::from_io(APPLET, "reading stdin", None, e)]);
        }
    } else {
        for (idx, path) in paths.iter().enumerate() {
            if (multiple || verbose) && !quiet {
                if idx > 0 {
                    writeln!(out)
                        .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
                }
                let label = if *path == "-" { "standard input" } else { path };
                writeln!(out, "==> {label} <==")
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            }
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
                Ok(mut input) => {
                    let result = if let Some(bc) = bytes {
                        head_bytes(&mut input, &mut out, bc)
                    } else {
                        head_lines(&mut BufReader::new(input), &mut out, n)
                    };
                    if let Err(e) = result {
                        errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                    }
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

fn parse_count(applet: &'static str, s: &str) -> Result<i64, Vec<AppletError>> {
    s.parse::<i64>()
        .map_err(|_| vec![AppletError::new(applet, format!("invalid count: '{s}'"))])
}

pub(crate) fn head_lines<R: BufRead, W: Write>(
    reader: &mut R,
    out: &mut W,
    n: i64,
) -> io::Result<()> {
    if n >= 0 {
        let mut remaining = n;
        let mut line = Vec::new();
        while remaining > 0 {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            out.write_all(&line)?;
            remaining -= 1;
        }
    } else {
        // -n -N: print all but the last N lines
        let skip = (-n) as usize;
        let mut ring: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            ring.push_back(line.clone());
        }
        if ring.len() > skip {
            for l in ring.iter().take(ring.len() - skip) {
                out.write_all(l)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn head_bytes<R: Read, W: Write>(reader: &mut R, out: &mut W, n: i64) -> io::Result<()> {
    if n >= 0 {
        let mut remaining = n as u64;
        let mut buf = [0u8; BUFFER_SIZE];
        while remaining > 0 {
            let to_read = (buf.len() as u64).min(remaining) as usize;
            let got = reader.read(&mut buf[..to_read])?;
            if got == 0 {
                break;
            }
            out.write_all(&buf[..got])?;
            remaining -= got as u64;
        }
    } else {
        // -c -N: print all but the last N bytes
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let skip = (-n) as usize;
        if data.len() > skip {
            out.write_all(&data[..data.len() - skip])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{head_bytes, head_lines};
    use std::io::{BufReader, Cursor};

    fn lines(input: &str, n: i64) -> String {
        let mut out = Vec::new();
        head_lines(&mut BufReader::new(Cursor::new(input)), &mut out, n).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn bytes(input: &str, n: i64) -> String {
        let mut out = Vec::new();
        head_bytes(&mut Cursor::new(input), &mut out, n).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn first_n_lines() {
        assert_eq!(lines("a\nb\nc\nd\n", 2), "a\nb\n");
    }

    #[test]
    fn first_n_lines_fewer_than_n() {
        assert_eq!(lines("a\nb\n", 10), "a\nb\n");
    }

    #[test]
    fn zero_lines() {
        assert_eq!(lines("a\nb\n", 0), "");
    }

    #[test]
    fn all_but_last_n_lines() {
        assert_eq!(lines("a\nb\nc\nd\n", -1), "a\nb\nc\n");
        assert_eq!(lines("a\nb\nc\nd\n", -2), "a\nb\n");
    }

    #[test]
    fn all_but_last_n_more_than_total() {
        assert_eq!(lines("a\nb\n", -5), "");
    }

    #[test]
    fn first_n_bytes() {
        assert_eq!(bytes("hello world", 5), "hello");
    }

    #[test]
    fn all_but_last_n_bytes() {
        assert_eq!(bytes("hello", -2), "hel");
    }
}
