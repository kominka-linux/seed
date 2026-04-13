use std::ffi::CString;
use std::io::Write;

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "sysctl";
// TODO: Replace this temporary BSD sysctl-by-name implementation with the
// Linux /proc/sys-backed behavior this project actually targets.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    values_only: bool,
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
    let (options, names) = parse_args(args)?;
    let mut out = stdout();

    for name in names {
        let value = read_value(&name)?;
        if options.values_only {
            writeln!(out, "{value}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        } else {
            writeln!(out, "{name}: {value}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut names = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "-n" {
            options.values_only = true;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }
        names.push(arg.clone());
    }

    if names.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((options, names))
}

fn read_value(name: &str) -> Result<String, Vec<AppletError>> {
    let c_name = CString::new(name)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid name '{name}'"))])?;
    let mut size = 0_usize;

    // SAFETY: `c_name` is NUL-terminated and `size` points to writable memory.
    let rc = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown key '{name}'"),
        )]);
    }

    let mut buffer = vec![0_u8; size];
    // SAFETY: `buffer` points to `size` writable bytes for sysctl to fill.
    let rc = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown key '{name}'"),
        )]);
    }
    buffer.truncate(size);
    Ok(format_value(&buffer))
}

fn format_value(bytes: &[u8]) -> String {
    if bytes.last() == Some(&0)
        && bytes[..bytes.len() - 1]
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
    {
        return String::from_utf8_lossy(&bytes[..bytes.len() - 1]).into_owned();
    }

    match bytes.len() {
        1 => i8::from_ne_bytes([bytes[0]]).to_string(),
        2 => i16::from_ne_bytes([bytes[0], bytes[1]]).to_string(),
        4 => i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).to_string(),
        8 => i64::from_ne_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
        .to_string(),
        _ => bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(""),
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, format_value, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_n_and_names() {
        let (options, names) =
            parse_args(&args(&["-n", "kern.ostype", "hw.ncpu"])).expect("parse sysctl");
        assert_eq!(options, Options { values_only: true });
        assert_eq!(
            names,
            vec![String::from("kern.ostype"), String::from("hw.ncpu")]
        );
    }

    #[test]
    fn formats_strings_and_integers() {
        assert_eq!(format_value(b"Darwin\0"), "Darwin");
        assert_eq!(format_value(&10_i32.to_ne_bytes()), "10");
    }
}
