use std::borrow::Cow;
use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "dmesg";
const DEFAULT_BUFFER_SIZE: i32 = 16 * 1024;

const SYSLOG_ACTION_READ_ALL: libc::c_int = 3;
const SYSLOG_ACTION_READ_CLEAR: libc::c_int = 4;
const SYSLOG_ACTION_CONSOLE_LEVEL: libc::c_int = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    clear_after_read: bool,
    raw: bool,
    console_level: Option<i32>,
    buffer_size: i32,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    run_linux(options)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options {
        clear_after_read: false,
        raw: false,
        console_level: None,
        buffer_size: DEFAULT_BUFFER_SIZE,
    };
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg(APPLET)? {
        match token {
            ArgToken::LongOption("clear", None) => options.clear_after_read = true,
            ArgToken::LongOption("raw", None) => options.raw = true,
            ArgToken::LongOption("console-level", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "n")?;
                options.console_level = Some(parse_number(value)?);
            }
            ArgToken::LongOption("buffer-size", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "s")?;
                options.buffer_size = parse_number(value)?;
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => parse_short_flags(flags, &mut cursor, &mut options)?,
            ArgToken::Operand(_) => {}
        }
    }

    Ok(options)
}

fn parse_short_flags<'a>(
    flags: &'a str,
    cursor: &mut ArgCursor<'a, std::ffi::OsString>,
    options: &mut Options,
) -> Result<(), Vec<AppletError>> {
    for (index, flag) in flags.char_indices() {
        match flag {
            'c' => options.clear_after_read = true,
            'r' => options.raw = true,
            'n' => {
                let attached = &flags[index + flag.len_utf8()..];
                let value = cursor.next_value_or_attached(attached, APPLET, "n")?;
                options.console_level = Some(parse_number(value)?);
                break;
            }
            's' => {
                let attached = &flags[index + flag.len_utf8()..];
                let value = cursor.next_value_or_attached(attached, APPLET, "s")?;
                options.buffer_size = parse_number(value)?;
                break;
            }
            _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
        }
    }

    Ok(())
}

fn parse_number(value: &str) -> Result<i32, Vec<AppletError>> {
    let parsed = value
        .parse::<i32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid number '{value}'"))])?;
    if parsed < 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid number '{value}'"),
        )]);
    }
    Ok(parsed)
}

fn run_linux(options: Options) -> AppletResult {
    if let Some(level) = options.console_level {
        return klogctl(SYSLOG_ACTION_CONSOLE_LEVEL, std::ptr::null_mut(), level)
            .map(|_| ())
            .map_err(|err| vec![AppletError::new(APPLET, format!("klogctl: {err}"))]);
    }

    let mut buffer = vec![0_u8; options.buffer_size.max(1) as usize];
    let action = if options.clear_after_read {
        SYSLOG_ACTION_READ_CLEAR
    } else {
        SYSLOG_ACTION_READ_ALL
    };
    let read = klogctl(action, buffer.as_mut_ptr().cast(), options.buffer_size)
        .map_err(|err| vec![AppletError::new(APPLET, format!("klogctl: {err}"))])?;

    let text = String::from_utf8_lossy(&buffer[..read as usize]);
    let output = if options.raw {
        text
    } else {
        strip_syslog_prefixes(&text)
    };

    let mut out = stdout();
    write!(out, "{output}")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn klogctl(
    action: libc::c_int,
    buffer: *mut libc::c_char,
    length: libc::c_int,
) -> Result<libc::c_int, std::io::Error> {
    // SAFETY: the action code is chosen by this module; when a buffer is
    // provided it points to writable memory for at least `length` bytes.
    let rc = unsafe { libc::klogctl(action, buffer, length) };
    if rc >= 0 {
        Ok(rc)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn strip_syslog_prefixes(text: &str) -> Cow<'_, str> {
    if !text.contains('<') {
        return Cow::Borrowed(text);
    }

    let mut normalized = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        normalized.push_str(strip_syslog_prefix(line));
    }

    Cow::Owned(normalized)
}

fn strip_syslog_prefix(line: &str) -> &str {
    let Some(rest) = line.strip_prefix('<') else {
        return line;
    };
    let digits = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits == 0 || rest.as_bytes().get(digits) != Some(&b'>') {
        return line;
    }
    &rest[digits + 1..]
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{Options, parse_args, parse_number};
    use super::{strip_syslog_prefix, strip_syslog_prefixes};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_supported_flags() {
        assert_eq!(
            parse_args(&args(&["-cr", "-n", "5", "-s32", "ignored"])).expect("parse dmesg"),
            Options {
                clear_after_read: true,
                raw: true,
                console_level: Some(5),
                buffer_size: 32,
            }
        );
    }

    #[test]
    fn rejects_invalid_numbers() {
        assert!(parse_number("x").is_err());
        assert!(parse_number("-1").is_err());
    }

    #[test]
    fn strips_prefix_from_kernel_messages() {
        assert_eq!(strip_syslog_prefix("<6>[1.0] hello\n"), "[1.0] hello\n");
        assert_eq!(strip_syslog_prefix("[1.0] hello\n"), "[1.0] hello\n");
    }

    #[test]
    fn strips_prefixes_across_multiple_lines() {
        assert_eq!(
            strip_syslog_prefixes("<6>one\n<4>two\nplain\n").as_ref(),
            "one\ntwo\nplain\n"
        );
    }
}
