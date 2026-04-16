use std::io::Read;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, open_input};

const APPLET: &str = "wc";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    bytes: bool,
    lines: bool,
    words: bool,
    longest_line: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    lines: u64,
    words: u64,
    bytes: u64,
    longest_line: u64,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (mut options, files) = parse_args(&args)?;
    if !options.bytes && !options.lines && !options.words && !options.longest_line {
        options.lines = true;
        options.words = true;
        options.bytes = true;
    }

    let inputs = if files.is_empty() {
        vec![String::from("-")]
    } else {
        files
    };

    let mut output = String::new();
    let mut errors = Vec::new();
    let mut total = Counts::default();
    let multiple = inputs.len() > 1;

    for path in &inputs {
        match count_path(path) {
            Ok(counts) => {
                total = add_counts(total, counts);
                output.push_str(&format_counts(options, counts, Some(path), multiple));
            }
            Err(err) => errors.push(AppletError::from_io(APPLET, "reading", Some(path), err)),
        }
    }

    if multiple {
        output.push_str(&format_counts(options, total, Some("total"), true));
    }

    print!("{output}");

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'c' => options.bytes = true,
                    'l' => options.lines = true,
                    'w' => options.words = true,
                    'L' => options.longest_line = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        files.push(arg.clone());
    }

    Ok((options, files))
}

fn count_path(path: &str) -> std::io::Result<Counts> {
    let mut input = open_input(path)?;
    let mut buf = [0_u8; BUFFER_SIZE];
    let mut counts = Counts::default();
    let mut in_word = false;
    let mut current_line = 0_u64;

    loop {
        let read = input.read(&mut buf)?;
        if read == 0 {
            counts.longest_line = counts.longest_line.max(current_line);
            return Ok(counts);
        }
        counts.bytes += read as u64;
        for &byte in &buf[..read] {
            if byte == b'\n' {
                counts.lines += 1;
                counts.longest_line = counts.longest_line.max(current_line);
                current_line = 0;
            } else {
                current_line += 1;
            }

            if byte.is_ascii_whitespace() {
                in_word = false;
            } else if !in_word {
                counts.words += 1;
                in_word = true;
            }
        }
    }
}

fn format_counts(
    options: Options,
    counts: Counts,
    name: Option<&str>,
    include_name: bool,
) -> String {
    let mut columns = Vec::new();
    if options.lines {
        columns.push(counts.lines.to_string());
    }
    if options.words {
        columns.push(counts.words.to_string());
    }
    if options.bytes {
        columns.push(counts.bytes.to_string());
    }
    if options.longest_line {
        columns.push(counts.longest_line.to_string());
    }

    let mut line = columns.join(" ");
    if include_name
        && let Some(name) = name
        && name != "-"
    {
        line.push(' ');
        line.push_str(name);
    }
    line.push('\n');
    line
}

fn add_counts(lhs: Counts, rhs: Counts) -> Counts {
    Counts {
        lines: lhs.lines + rhs.lines,
        words: lhs.words + rhs.words,
        bytes: lhs.bytes + rhs.bytes,
        longest_line: lhs.longest_line.max(rhs.longest_line),
    }
}
