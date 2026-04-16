use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, open_input};

const APPLET: &str = "split";
const DEFAULT_LINES: usize = 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Lines(usize),
    Bytes(usize),
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (mode, input, prefix) = parse_args(&args)?;
    match mode {
        Mode::Lines(lines) => split_lines(&input, &prefix, lines),
        Mode::Bytes(bytes) => split_bytes(&input, &prefix, bytes),
    }
}

fn parse_args(args: &[String]) -> Result<(Mode, String, String), Vec<AppletError>> {
    let mut mode = Mode::Lines(DEFAULT_LINES);
    let mut paths = Vec::new();
    let mut index = 0;
    let mut parsing_flags = true;

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-l" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "l")]);
            };
            mode = Mode::Lines(parse_positive(value)?);
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-b" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "b")]);
            };
            mode = Mode::Bytes(parse_positive(value)?);
            index += 1;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        paths.push(arg.clone());
        index += 1;
    }

    let (input, prefix) = match paths.as_slice() {
        [] => (String::from("-"), String::from("x")),
        [input] => (input.clone(), String::from("x")),
        [input, prefix] => (input.clone(), prefix.clone()),
        _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
    };

    Ok((mode, input, prefix))
}

fn parse_positive(text: &str) -> Result<usize, Vec<AppletError>> {
    match text.parse::<usize>() {
        Ok(0) | Err(_) => Err(vec![AppletError::new(
            APPLET,
            format!("invalid number '{text}'"),
        )]),
        Ok(value) => Ok(value),
    }
}

fn split_lines(input: &str, prefix: &str, lines_per_file: usize) -> AppletResult {
    let reader = open_input(input)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(input), err)])?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, reader);
    let mut line = Vec::new();
    let mut writer = None;
    let mut index = 0_usize;
    let mut line_count = 0_usize;

    loop {
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(input), err)])?;
        if bytes == 0 {
            break;
        }
        if writer.is_none() || line_count == lines_per_file {
            writer = Some(create_output(prefix, index)?);
            index += 1;
            line_count = 0;
        }
        writer
            .as_mut()
            .expect("writer exists")
            .write_all(&line)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        line_count += 1;
    }

    Ok(())
}

fn split_bytes(input: &str, prefix: &str, bytes_per_file: usize) -> AppletResult {
    let mut reader = open_input(input)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(input), err)])?;
    let mut buffer = vec![0_u8; bytes_per_file];
    let mut index = 0_usize;

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(input), err)])?;
        if read == 0 {
            break;
        }
        let mut writer = create_output(prefix, index)?;
        writer
            .write_all(&buffer[..read])
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        index += 1;
    }

    Ok(())
}

fn create_output(prefix: &str, index: usize) -> Result<BufWriter<File>, Vec<AppletError>> {
    let suffix = suffix(index).ok_or_else(|| vec![AppletError::new(APPLET, "too many files")])?;
    let path = format!("{prefix}{suffix}");
    let file = File::create(&path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "creating", Some(&path), err)])?;
    Ok(BufWriter::with_capacity(BUFFER_SIZE, file))
}

fn suffix(index: usize) -> Option<String> {
    if index >= 26 * 26 {
        return None;
    }
    let first = ((index / 26) as u8 + b'a') as char;
    let second = ((index % 26) as u8 + b'a') as char;
    Some(format!("{first}{second}"))
}

#[cfg(test)]
mod tests {
    use super::{Mode, parse_args, suffix};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_line_mode_and_prefix() {
        let (mode, input, prefix) =
            parse_args(&args(&["-l", "2", "input", "out"])).expect("parse split");
        assert_eq!(mode, Mode::Lines(2));
        assert_eq!(input, "input");
        assert_eq!(prefix, "out");
    }

    #[test]
    fn generates_two_letter_suffixes() {
        assert_eq!(suffix(0).as_deref(), Some("aa"));
        assert_eq!(suffix(27).as_deref(), Some("bb"));
    }
}
