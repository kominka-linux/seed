use std::io::{self, BufReader, Read, Write};

use crate::common::applet::finish_code_or;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, open_input, stdout};

const APPLET: &str = "cmp";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    silent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Comparison {
    Same,
    Different { byte: usize, line: usize },
}

pub fn main(args: &[String]) -> i32 {
    finish_code_or(
        run(args).map(|comparison| match comparison {
            Comparison::Same => 0,
            Comparison::Different { .. } => 1,
        }),
        2,
    )
}

fn run(args: &[String]) -> Result<Comparison, Vec<AppletError>> {
    let (options, left, right) = parse_args(args)?;
    if left == "-" && right == "-" {
        return Err(vec![AppletError::new(
            APPLET,
            "standard input may only be specified once",
        )]);
    }

    let left_input = open_input(&left)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&left), err)])?;
    let right_input = open_input(&right)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&right), err)])?;
    let mut left_reader = BufReader::with_capacity(BUFFER_SIZE, left_input);
    let mut right_reader = BufReader::with_capacity(BUFFER_SIZE, right_input);

    match compare_readers(&mut left_reader, &mut right_reader)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?
    {
        Comparison::Same => Ok(Comparison::Same),
        Comparison::Different { byte, line } => {
            if !options.silent {
                let mut out = stdout();
                writeln!(out, "{left} {right} differ: byte {byte}, line {line}").map_err(
                    |err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)],
                )?;
                out.flush().map_err(|err| {
                    vec![AppletError::from_io(APPLET, "writing stdout", None, err)]
                })?;
            }
            Ok(Comparison::Different { byte, line })
        }
    }
}

fn parse_args(args: &[String]) -> Result<(Options, String, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            for flag in arg[1..].chars() {
                match flag {
                    's' => options.silent = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    match paths.as_slice() {
        [left, right] => Ok((options, left.clone(), right.clone())),
        [] | [_] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn compare_readers<R: Read, W: Read>(left: &mut R, right: &mut W) -> io::Result<Comparison> {
    let mut left_buf = [0_u8; BUFFER_SIZE];
    let mut right_buf = [0_u8; BUFFER_SIZE];
    let mut byte = 1_usize;
    let mut line = 1_usize;
    let mut left_len = 0;
    let mut right_len = 0;
    let mut left_pos = 0;
    let mut right_pos = 0;

    loop {
        if left_pos == left_len {
            left_len = left.read(&mut left_buf)?;
            left_pos = 0;
        }
        if right_pos == right_len {
            right_len = right.read(&mut right_buf)?;
            right_pos = 0;
        }

        if left_pos == left_len && right_pos == right_len {
            return Ok(Comparison::Same);
        }
        if left_pos == left_len || right_pos == right_len {
            return Ok(Comparison::Different { byte, line });
        }

        let left_byte = left_buf[left_pos];
        let right_byte = right_buf[right_pos];
        if left_byte != right_byte {
            return Ok(Comparison::Different { byte, line });
        }

        if left_byte == b'\n' {
            line += 1;
        }
        byte += 1;
        left_pos += 1;
        right_pos += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{Comparison, compare_readers, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_silent_option() {
        let (options, left, right) =
            parse_args(&args(&["-s", "left", "right"])).expect("parse cmp");
        assert!(options.silent);
        assert_eq!(left, "left");
        assert_eq!(right, "right");
    }

    #[test]
    fn compare_readers_reports_first_difference() {
        let mut left = Cursor::new(b"abc\nxyz".to_vec());
        let mut right = Cursor::new(b"abc\nxqz".to_vec());
        assert_eq!(
            compare_readers(&mut left, &mut right).expect("compare"),
            Comparison::Different { byte: 6, line: 2 }
        );
    }

    #[test]
    fn compare_readers_reports_eof_difference() {
        let mut left = Cursor::new(b"abc".to_vec());
        let mut right = Cursor::new(b"abcd".to_vec());
        assert_eq!(
            compare_readers(&mut left, &mut right).expect("compare"),
            Comparison::Different { byte: 4, line: 1 }
        );
    }
}
