use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "echo";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut newline = true;
    let mut escape = false;
    let mut start = 0;

    // Consume leading flag arguments (-n, -e, -E and combinations like -ne).
    'flags: for (i, arg) in args.iter().enumerate() {
        if !arg.starts_with('-') || arg.len() == 1 {
            start = i;
            break 'flags;
        }
        for ch in arg[1..].chars() {
            match ch {
                'n' => newline = false,
                'e' => escape = true,
                'E' => escape = false,
                _ => {
                    start = i;
                    break 'flags;
                }
            }
        }
        start = i + 1;
    }

    let parts = &args[start..];
    let mut out = stdout();
    let mut stop = false;

    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.write_all(b" ")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
        if escape {
            let expanded = expand_escapes(part.as_bytes(), &mut stop);
            out.write_all(&expanded)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            if stop {
                return Ok(());
            }
        } else {
            out.write_all(part.as_bytes())
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
    }

    if newline {
        out.write_all(b"\n")
            .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    }
    Ok(())
}

fn expand_escapes(input: &[u8], stop: &mut bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] != b'\\' || i + 1 >= input.len() {
            out.push(input[i]);
            i += 1;
            continue;
        }
        i += 1;
        match input[i] {
            b'\\' => { out.push(b'\\'); i += 1; }
            b'n'  => { out.push(b'\n'); i += 1; }
            b't'  => { out.push(b'\t'); i += 1; }
            b'r'  => { out.push(b'\r'); i += 1; }
            b'a'  => { out.push(0x07);  i += 1; }
            b'b'  => { out.push(0x08);  i += 1; }
            b'f'  => { out.push(0x0C);  i += 1; }
            b'v'  => { out.push(0x0B);  i += 1; }
            b'c'  => { *stop = true; return out; }
            b'0'  => {
                // \0NNN: 1-3 octal digits
                i += 1;
                let mut val: u32 = 0;
                let mut count = 0;
                while count < 3 && i < input.len() && input[i] >= b'0' && input[i] <= b'7' {
                    val = val * 8 + u32::from(input[i] - b'0');
                    i += 1;
                    count += 1;
                }
                out.push(val as u8);
            }
            b'x'  => {
                // \xHH: 1-2 hex digits
                i += 1;
                let mut val: u8 = 0;
                let mut count = 0;
                while count < 2 && i < input.len() {
                    let digit = match input[i] {
                        b'0'..=b'9' => input[i] - b'0',
                        b'a'..=b'f' => input[i] - b'a' + 10,
                        b'A'..=b'F' => input[i] - b'A' + 10,
                        _ => break,
                    };
                    val = val * 16 + digit;
                    i += 1;
                    count += 1;
                }
                out.push(val);
            }
            other => {
                // Unknown escape: pass through as-is
                out.push(b'\\');
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::expand_escapes;

    #[test]
    fn basic_escapes() {
        let mut stop = false;
        assert_eq!(expand_escapes(b"hello\\nworld", &mut stop), b"hello\nworld");
        assert_eq!(expand_escapes(b"\\t", &mut stop), b"\t");
        assert_eq!(expand_escapes(b"\\\\", &mut stop), b"\\");
    }

    #[test]
    fn octal_escape() {
        let mut stop = false;
        assert_eq!(expand_escapes(b"\\0101", &mut stop), b"A"); // 0101 octal = 65 decimal = 'A'
    }

    #[test]
    fn hex_escape() {
        let mut stop = false;
        assert_eq!(expand_escapes(b"\\x41", &mut stop), b"A");
    }

    #[test]
    fn stop_at_c() {
        let mut stop = false;
        let out = expand_escapes(b"hello\\cworld", &mut stop);
        assert_eq!(out, b"hello");
        assert!(stop);
    }
}
