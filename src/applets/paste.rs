use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::Parser;
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "paste";

#[derive(Clone, Debug)]
struct Options {
    serial: bool,
    delimiters: Vec<u8>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let (options, files) = parse_args(args)?;
    let inputs = if files.is_empty() {
        vec![String::from("-")]
    } else {
        files
    };
    let mut out = stdout();

    if options.serial {
        for path in inputs {
            let input = open_input(&path)
                .map_err(|e| vec![AppletError::from_io(APPLET, "opening", Some(&path), e)])?;
            paste_serial(BufReader::new(input), &mut out, &options)
                .map_err(|e| vec![AppletError::from_io(APPLET, "reading", Some(&path), e)])?;
        }
    } else {
        let mut readers = Vec::new();
        for path in &inputs {
            let input = open_input(path)
                .map_err(|e| vec![AppletError::from_io(APPLET, "opening", Some(path), e)])?;
            readers.push(BufReader::new(input));
        }
        paste_parallel(&mut readers, &mut out, &options)?;
    }

    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut serial = false;
    let mut delimiters = vec![b'\t'];
    let mut files = Vec::new();
    let mut parser = Parser::new(APPLET, args);
    let mut args = parser.raw_args()?;

    while let Some(arg) = args.next() {
        let arg = to_string(APPLET, arg)?;
        match arg.as_str() {
            "--" => {
                for value in args.by_ref() {
                    files.push(to_string(APPLET, value)?);
                }
                break;
            }
            "-s" => serial = true,
            "-d" => {
                let Some(value) = args.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "d")]);
                };
                delimiters = parse_delimiters(&to_string(APPLET, value)?);
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => files.push(arg),
        }
    }

    Ok((Options { serial, delimiters }, files))
}

fn to_string(applet: &'static str, value: OsString) -> Result<String, Vec<AppletError>> {
    value.into_string().map_err(|value| {
        vec![AppletError::new(
            applet,
            format!("argument is invalid unicode: {:?}", value),
        )]
    })
}

fn parse_delimiters(input: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('t') => out.push(b'\t'),
                Some('n') => out.push(b'\n'),
                Some('0') => out.push(0),
                Some('\\') => out.push(b'\\'),
                Some(other) => out.extend_from_slice(other.to_string().as_bytes()),
                None => out.push(b'\\'),
            }
        } else {
            out.extend_from_slice(ch.to_string().as_bytes());
        }
    }
    if out.is_empty() { vec![b'\t'] } else { out }
}

fn paste_parallel<R: BufRead, W: Write>(
    readers: &mut [R],
    out: &mut W,
    options: &Options,
) -> AppletResult {
    let mut lines = vec![Vec::new(); readers.len()];

    loop {
        let mut any = false;
        for (idx, reader) in readers.iter_mut().enumerate() {
            lines[idx].clear();
            let read = reader
                .read_until(b'\n', &mut lines[idx])
                .map_err(|e| vec![AppletError::from_io(APPLET, "reading", None, e)])?;
            if read > 0 {
                any = true;
                if lines[idx].last() == Some(&b'\n') {
                    lines[idx].pop();
                }
            }
        }

        if !any {
            break;
        }

        for (idx, line) in lines.iter().enumerate() {
            if idx > 0 {
                out.write_all(&[delimiter_at(&options.delimiters, idx - 1)])
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            }
            out.write_all(line)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
        out.write_all(b"\n")
            .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    }

    Ok(())
}

fn paste_serial<R: BufRead, W: Write>(
    mut reader: R,
    out: &mut W,
    options: &Options,
) -> io::Result<()> {
    let mut first = true;
    let mut index = 0;
    let mut line = Vec::new();

    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if !first {
            out.write_all(&[delimiter_at(&options.delimiters, index)])?;
            index += 1;
        }
        out.write_all(&line)?;
        first = false;
    }

    out.write_all(b"\n")
}

fn delimiter_at(delimiters: &[u8], index: usize) -> u8 {
    delimiters[index % delimiters.len()]
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::{Options, parse_delimiters, paste_parallel, paste_serial};

    #[test]
    fn parses_common_delimiters() {
        assert_eq!(parse_delimiters("\\t:"), vec![b'\t', b':']);
    }

    #[test]
    fn parallel_mode_joins_lines() {
        let mut readers = [
            BufReader::new("a1\na2\n".as_bytes()),
            BufReader::new("b1\nb2\n".as_bytes()),
        ];
        let mut out = Vec::new();
        paste_parallel(
            &mut readers,
            &mut out,
            &Options {
                serial: false,
                delimiters: vec![b'\t'],
            },
        )
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a1\tb1\na2\tb2\n");
    }

    #[test]
    fn serial_mode_joins_single_file() {
        let mut out = Vec::new();
        paste_serial(
            BufReader::new("a1\na2\n".as_bytes()),
            &mut out,
            &Options {
                serial: true,
                delimiters: vec![b':'],
            },
        )
        .unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "a1:a2\n");
    }
}
