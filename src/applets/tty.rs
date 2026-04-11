use std::ffi::CStr;
use std::io::Write;

use crate::common::io::stdout;

const APPLET: &str = "tty";

pub fn main(args: &[String]) -> i32 {
    let mut silent = false;
    for arg in args {
        match arg.as_str() {
            "-s" | "--silent" | "--quiet" => silent = true,
            "--" => break,
            a if a.starts_with('-') => {
                eprintln!("{APPLET}: invalid option -- '{}'", &a[1..]);
                return 1;
            }
            _ => {}
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

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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
