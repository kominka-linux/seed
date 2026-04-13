use std::fs::File;
use std::io::{self, BufReader, Read, Write};

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, copy_stream, stdout};

const APPLET: &str = "cat";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    number_nonblank: bool,
    number_all: bool,
    show_nonprinting: bool,
    show_ends: bool,
    show_tabs: bool,
}

impl Options {
    fn requires_transform(self) -> bool {
        self.number_nonblank
            || self.number_all
            || self.show_nonprinting
            || self.show_ends
            || self.show_tabs
    }
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let (options, files) = parse_args(args)?;
    let mut out = stdout();
    let inputs: Vec<&str> = if files.is_empty() {
        vec!["-"]
    } else {
        files.iter().map(String::as_str).collect()
    };

    let mut state = RenderState::default();
    let mut errors = Vec::new();

    for path in inputs {
        let result = if path == "-" {
            let stdin = io::stdin();
            let mut locked = stdin.lock();
            process_reader(&mut locked, &mut out, options, &mut state)
        } else {
            match File::open(path) {
                Ok(file) => {
                    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
                    process_reader(&mut reader, &mut out, options, &mut state)
                }
                Err(err) => Err(AppletError::from_io(APPLET, "opening", Some(path), err)),
            }
        };

        if let Err(err) = result {
            errors.push(err);
        }
    }

    if let Err(err) = out.flush() {
        errors.push(AppletError::from_io(APPLET, "writing stdout", None, err));
    }

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
            for ch in arg[1..].chars() {
                match ch {
                    'A' => {
                        options.show_nonprinting = true;
                        options.show_ends = true;
                        options.show_tabs = true;
                    }
                    'b' => options.number_nonblank = true,
                    'e' => {
                        options.show_nonprinting = true;
                        options.show_ends = true;
                    }
                    'n' => options.number_all = true,
                    't' => {
                        options.show_nonprinting = true;
                        options.show_tabs = true;
                    }
                    'v' => options.show_nonprinting = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, ch)]),
                }
            }
            continue;
        }

        files.push(arg.clone());
    }

    if options.number_nonblank {
        options.number_all = false;
    }

    Ok((options, files))
}

fn process_reader<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    options: Options,
    state: &mut RenderState,
) -> Result<(), AppletError> {
    if !options.requires_transform() {
        return copy_stream(reader, writer)
            .map(|_| ())
            .map_err(|err| AppletError::from_io(APPLET, "reading", None, err));
    }

    let mut buf = [0_u8; BUFFER_SIZE];

    loop {
        let read = reader
            .read(&mut buf)
            .map_err(|err| AppletError::from_io(APPLET, "reading", None, err))?;
        if read == 0 {
            return Ok(());
        }

        for &byte in &buf[..read] {
            render_byte(byte, writer, options, state)
                .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))?;
        }
    }
}

#[derive(Debug)]
struct RenderState {
    next_line_number: usize,
    at_line_start: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            next_line_number: 1,
            at_line_start: true,
        }
    }
}

fn render_byte<W: Write>(
    byte: u8,
    writer: &mut W,
    options: Options,
    state: &mut RenderState,
) -> io::Result<()> {
    if state.at_line_start {
        let is_nonblank = byte != b'\n';
        if options.number_nonblank {
            if is_nonblank {
                write_line_number(writer, state.next_line_number)?;
                state.next_line_number += 1;
            }
        } else if options.number_all {
            write_line_number(writer, state.next_line_number)?;
            state.next_line_number += 1;
        }
        state.at_line_start = false;
    }

    match byte {
        b'\n' => {
            if options.show_ends {
                writer.write_all(b"$")?;
            }
            writer.write_all(b"\n")?;
            state.at_line_start = true;
        }
        b'\t' if options.show_tabs => {
            writer.write_all(b"^I")?;
        }
        _ if options.show_nonprinting => {
            write_visible_byte(writer, byte)?;
        }
        _ => {
            writer.write_all(&[byte])?;
        }
    }

    Ok(())
}

fn write_line_number<W: Write>(writer: &mut W, line: usize) -> io::Result<()> {
    write!(writer, "{line:>6}\t")
}

fn write_visible_byte<W: Write>(writer: &mut W, byte: u8) -> io::Result<()> {
    match byte {
        0x00..=0x08 | 0x0B..=0x1F => {
            let caret = byte + 64;
            writer.write_all(&[b'^', caret])
        }
        0x7F => writer.write_all(b"^?"),
        0x80..=0x9F => {
            let caret = byte - 64;
            writer.write_all(&[b'M', b'-', b'^', caret])
        }
        0xA0..=0xFE => {
            let visible = byte - 128;
            writer.write_all(b"M-")?;
            write_visible_byte(writer, visible)
        }
        0xFF => writer.write_all(b"M-^?"),
        _ => writer.write_all(&[byte]),
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, RenderState, parse_args, process_reader};

    #[test]
    fn parse_combined_flags() {
        let args = vec![String::from("-Aben"), String::from("file")];
        let (options, files) = parse_args(&args).expect("parse args");

        assert!(options.number_nonblank);
        assert!(!options.number_all);
        assert!(options.show_nonprinting);
        assert!(options.show_ends);
        assert!(options.show_tabs);
        assert_eq!(files, vec![String::from("file")]);
    }

    #[test]
    fn number_nonblank_skips_empty_lines() {
        let mut output = Vec::new();
        let mut state = RenderState::default();
        let mut input = &b"line 1\n\nline 3\n"[..];

        process_reader(
            &mut input,
            &mut output,
            Options {
                number_nonblank: true,
                ..Options::default()
            },
            &mut state,
        )
        .expect("render");

        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "     1\tline 1\n\n     2\tline 3\n"
        );
    }

    #[test]
    fn show_all_marks_tabs_and_line_ends() {
        let mut output = Vec::new();
        let mut state = RenderState::default();
        let mut input = &b"a\tb\n"[..];

        process_reader(
            &mut input,
            &mut output,
            Options {
                show_nonprinting: true,
                show_ends: true,
                show_tabs: true,
                ..Options::default()
            },
            &mut state,
        )
        .expect("render");

        assert_eq!(String::from_utf8(output).expect("utf8"), "a^Ib$\n");
    }
}
