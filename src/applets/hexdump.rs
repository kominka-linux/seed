use std::io::{self, BufReader, Read, Write};

use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, open_input, stdout};

const APPLET: &str = "hexdump";
const LINE_WIDTH: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    no_squeeze: bool,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let (options, path) = parse_args(args)?;
    let input = open_input(&path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&path), err)])?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, input);
    let mut out = stdout();
    dump_canonical(&mut reader, &mut out, options.no_squeeze)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut parsing_flags = true;
    let mut paths = Vec::new();

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            for flag in arg[1..].chars() {
                match flag {
                    'C' => {}
                    'v' => options.no_squeeze = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    match paths.as_slice() {
        [] => Ok((options, String::from("-"))),
        [path] => Ok((options, path.clone())),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn dump_canonical<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    no_squeeze: bool,
) -> io::Result<()> {
    let mut buffer = [0_u8; LINE_WIDTH];
    let mut offset = 0_usize;
    let mut previous_full = None;
    let mut squeezed = false;

    loop {
        let filled = read_line(reader, &mut buffer)?;
        if filled == 0 {
            break;
        }

        let repeated_full = filled == LINE_WIDTH && previous_full == Some(buffer);
        if repeated_full && !no_squeeze {
            if !squeezed {
                writeln!(writer, "*")?;
                squeezed = true;
            }
        } else {
            write_canonical_line(writer, offset, &buffer[..filled])?;
            squeezed = false;
        }

        previous_full = if filled == LINE_WIDTH {
            Some(buffer)
        } else {
            None
        };
        offset += filled;
    }

    writeln!(writer, "{offset:08x}")?;
    Ok(())
}

fn read_line(reader: &mut impl Read, buffer: &mut [u8; LINE_WIDTH]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        let read = reader.read(&mut buffer[filled..])?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

fn write_canonical_line(writer: &mut impl Write, offset: usize, bytes: &[u8]) -> io::Result<()> {
    write!(writer, "{offset:08x}  ")?;
    for index in 0..LINE_WIDTH {
        if index < bytes.len() {
            write!(writer, "{:02x} ", bytes[index])?;
        } else {
            write!(writer, "   ")?;
        }
        if index == 7 {
            write!(writer, " ")?;
        }
    }

    write!(writer, " |")?;
    for &byte in bytes {
        let ch = if matches!(byte, 0x20..=0x7e) {
            byte as char
        } else {
            '.'
        };
        write!(writer, "{ch}")?;
    }
    writeln!(writer, "|")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Options, dump_canonical, parse_args, write_canonical_line};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_v_and_c_flags() {
        let (options, path) = parse_args(&args(&["-vC", "file"])).expect("parse hexdump");
        assert_eq!(options, Options { no_squeeze: true });
        assert_eq!(path, "file");
    }

    #[test]
    fn canonical_line_formats_partial_input() {
        let mut output = Vec::new();
        write_canonical_line(&mut output, 0, b"Hello, world!\n").expect("write line");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "00000000  48 65 6c 6c 6f 2c 20 77  6f 72 6c 64 21 0a        |Hello, world!.|\n"
        );
    }

    #[test]
    fn canonical_output_squeezes_repeated_lines() {
        let mut output = Vec::new();
        let mut input = Cursor::new(vec![0_u8; 32]);
        dump_canonical(&mut input, &mut output, false).expect("dump");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n*\n00000020\n"
        );
    }

    #[test]
    fn canonical_output_keeps_repeated_lines_with_v() {
        let mut output = Vec::new();
        let mut input = Cursor::new(vec![0_u8; 32]);
        dump_canonical(&mut input, &mut output, true).expect("dump");
        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n00000010  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n00000020\n"
        );
    }
}
