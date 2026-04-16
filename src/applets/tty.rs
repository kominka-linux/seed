use std::ffi::CStr;
use std::io::Write;

use crate::common::args::{ParsedArg, Parser};
use crate::common::io::stdout;

const APPLET: &str = "tty";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    let mut silent = false;
    let mut parser = Parser::new(APPLET, args);
    while let Some(arg) = match parser.next_arg() {
        Ok(arg) => arg,
        Err(err) => {
            err[0].print();
            return 1;
        }
    } {
        match arg {
            ParsedArg::Short('s') => silent = true,
            ParsedArg::Long(name) if matches!(name.as_str(), "silent" | "quiet") => silent = true,
            ParsedArg::Short(flag) => {
                eprintln!("{APPLET}: invalid option -- '{flag}'");
                return 1;
            }
            ParsedArg::Long(name) => {
                eprintln!("{APPLET}: unrecognized option '--{name}'");
                return 1;
            }
            ParsedArg::Value(_) => {}
        }
    }

    // SAFETY: isatty with a valid file descriptor always returns 0 or 1.
    let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } != 0;

    if !silent {
        let mut out = stdout();
        if is_tty {
            writeln!(out, "{}", tty_name()).ok();
        } else {
            writeln!(out, "not a tty").ok();
        }
    }

    i32::from(!is_tty)
}

fn tty_name() -> String {
    let mut buf = vec![0u8; 256];
    // SAFETY: STDIN_FILENO is valid; buf points to buf.len() writable bytes.
    let ret = unsafe {
        libc::ttyname_r(
            libc::STDIN_FILENO,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
        )
    };
    if ret == 0 {
        // SAFETY: ttyname_r wrote a NUL-terminated string into buf on success.
        unsafe { CStr::from_ptr(buf.as_ptr() as *const libc::c_char) }
            .to_string_lossy()
            .into_owned()
    } else {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{main, tty_name};
    use std::ffi::OsString;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn parser_accepts_long_silent() {
        assert_eq!(main(&[OsString::from("--silent")]), i32::from(unsafe {
            libc::isatty(libc::STDIN_FILENO)
        } == 0));
    }

    #[test]
    fn tty_name_is_never_empty() {
        assert!(!tty_name().is_empty());
    }

    #[test]
    fn invalid_option_fails() {
        assert_eq!(main(&args(&["-x"])), 1);
    }
}
