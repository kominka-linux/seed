use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::thread;
use std::time::Duration;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout, BUFFER_SIZE};

const APPLET: &str = "tail";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut lines: Option<i64> = None;
    let mut bytes: Option<i64> = None;
    let mut follow = false;
    let mut quiet = false;
    let mut verbose = false;
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() { paths.push(&args[i]); i += 1; }
                break;
            }
            "-q" | "--quiet" | "--silent" => quiet = true,
            "-v" | "--verbose" => verbose = true,
            "-f" | "--follow" => follow = true,
            "-n" | "--lines" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "n")]);
                }
                lines = Some(parse_count(APPLET, &args[i])?);
            }
            "-c" | "--bytes" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "c")]);
                }
                bytes = Some(parse_count(APPLET, &args[i])?);
            }
            // -NUM shorthand: tail -5 ≡ tail -n -5 (last 5)
            a if a.starts_with('-') && a[1..].chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) => {
                let n: i64 = a[1..].parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid count: '{a}'"))]
                })?;
                lines = Some(-n); // negative = last N
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(APPLET, a.chars().nth(1).unwrap_or('-'))]);
            }
            _ => paths.push(arg),
        }
        i += 1;
    }

    let n = lines.unwrap_or(-10);
    let multiple = paths.len() > 1;
    let mut out = stdout();
    let mut errors = Vec::new();

    let process_input = |input: &mut dyn Read, out: &mut dyn Write| -> io::Result<()> {
        if let Some(bc) = bytes {
            tail_bytes(input, out, bc)
        } else {
            tail_lines(&mut BufReader::new(input), out, n)
        }
    };

    if paths.is_empty() {
        if verbose {
            writeln!(out, "==> standard input <==")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
        let stdin = io::stdin();
        if let Err(e) = process_input(&mut stdin.lock(), &mut out) {
            return Err(vec![AppletError::from_io(APPLET, "reading stdin", None, e)]);
        }
    } else {
        for (idx, path) in paths.iter().enumerate() {
            if (multiple || verbose) && !quiet {
                if idx > 0 {
                    writeln!(out, "")
                        .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
                }
                let label = if *path == "-" { "standard input" } else { path };
                writeln!(out, "==> {label} <==")
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            }
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
                Ok(mut input) => {
                    if let Err(e) = process_input(&mut input, &mut out) {
                        errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                    }
                }
            }
        }

        // -f: follow the last file by polling
        if follow && errors.is_empty() {
            if let Some(&path) = paths.last() {
                if path != "-" {
                    if let Ok(mut file) = std::fs::File::open(path) {
                        file.seek(io::SeekFrom::End(0)).ok();
                        let mut buf = [0u8; 4096];
                        loop {
                            match file.read(&mut buf) {
                                Ok(0) => thread::sleep(Duration::from_millis(200)),
                                Ok(n) => {
                                    if let Err(e) = out.write_all(&buf[..n]) {
                                        errors.push(AppletError::from_io(APPLET, "writing", None, e));
                                        break;
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                                Err(e) => {
                                    errors.push(AppletError::from_io(APPLET, "following", Some(path), e));
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

// Positive n = from line n (1-based); negative n = last |n| lines.
fn parse_count(applet: &'static str, s: &str) -> Result<i64, Vec<AppletError>> {
    if let Some(rest) = s.strip_prefix('+') {
        // +N means "from line N onwards" — store as positive
        rest.parse::<i64>().map_err(|_| {
            vec![AppletError::new(applet, format!("invalid count: '{s}'"))]
        })
    } else {
        // N or -N both mean "last N lines" — store as negative
        let n: i64 = s.trim_start_matches('-').parse().map_err(|_| {
            vec![AppletError::new(applet, format!("invalid count: '{s}'"))]
        })?;
        Ok(-n)
    }
}

fn tail_lines<R: BufRead, W: Write + ?Sized>(reader: &mut R, out: &mut W, n: i64) -> io::Result<()> {
    if n > 0 {
        // From line n (1-based): skip n-1 lines, then copy the rest.
        let skip = (n as usize).saturating_sub(1);
        let mut skipped = 0;
        let mut line = Vec::new();
        while skipped < skip {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 { return Ok(()); }
            skipped += 1;
        }
        let mut buf = [0u8; BUFFER_SIZE];
        loop {
            let got = reader.read(&mut buf)?;
            if got == 0 { break; }
            out.write_all(&buf[..got])?;
        }
    } else {
        // Last |n| lines via a ring buffer
        let cap = (-n) as usize;
        let mut ring: VecDeque<Vec<u8>> = VecDeque::with_capacity(cap.saturating_add(1));
        let mut line = Vec::new();
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 { break; }
            if cap > 0 {
                if ring.len() == cap { ring.pop_front(); }
                ring.push_back(line.clone());
            }
        }
        for l in &ring { out.write_all(l)?; }
    }
    Ok(())
}

fn tail_bytes<R: Read + ?Sized, W: Write + ?Sized>(reader: &mut R, out: &mut W, n: i64) -> io::Result<()> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    if n >= 0 {
        // From byte n (1-based)
        let skip = (n as usize).saturating_sub(1).min(data.len());
        out.write_all(&data[skip..])?;
    } else {
        // Last |n| bytes
        let start = data.len().saturating_sub((-n) as usize);
        out.write_all(&data[start..])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};
    use super::{tail_bytes, tail_lines};

    fn lines(input: &str, n: i64) -> String {
        let mut out = Vec::new();
        tail_lines(&mut BufReader::new(Cursor::new(input)), &mut out, n).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn bytes(input: &str, n: i64) -> String {
        let mut out = Vec::new();
        tail_bytes(&mut Cursor::new(input), &mut out, n).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn last_n_lines() {
        assert_eq!(lines("a\nb\nc\nd\n", -2), "c\nd\n");
    }

    #[test]
    fn last_n_fewer_than_available() {
        assert_eq!(lines("a\nb\n", -10), "a\nb\n");
    }

    #[test]
    fn last_zero_lines() {
        assert_eq!(lines("a\nb\n", 0), "");
    }

    #[test]
    fn from_line_n() {
        // n=2 means from line 2 onwards (1-based)
        assert_eq!(lines("a\nb\nc\nd\n", 2), "b\nc\nd\n");
        assert_eq!(lines("a\nb\nc\nd\n", 1), "a\nb\nc\nd\n");
    }

    #[test]
    fn last_n_bytes() {
        assert_eq!(bytes("hello", -3), "llo");
    }

    #[test]
    fn from_byte_n() {
        // n=3 means from byte 3 onwards (1-based)
        assert_eq!(bytes("hello", 3), "llo");
    }

    #[test]
    fn last_n_bytes_more_than_total() {
        assert_eq!(bytes("hi", -100), "hi");
    }
}
