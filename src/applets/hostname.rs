use std::ffi::{CStr, CString};
use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "hostname";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut file: Option<String> = None;
    let mut new_name: Option<String> = None;
    let mut short = false;
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('F') => file = Some(parser.value_str("F")?),
            ParsedArg::Long(name) if name == "file" => file = Some(parser.value_str("file")?),
            ParsedArg::Short('s') => short = true,
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => {
                let arg = arg.into_string().map_err(|arg| {
                    vec![AppletError::new(
                        APPLET,
                        format!("argument is invalid unicode: {:?}", arg),
                    )]
                })?;
                if new_name.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                new_name = Some(arg);
            }
        }
    }

    if let Some(path) = file.as_deref() {
        let content = std::fs::read_to_string(path)
            .map_err(|e| vec![AppletError::from_io(APPLET, "reading", Some(path), e)])?;
        let name = content.lines().next().unwrap_or("").trim();
        return set_hostname(name);
    }

    if let Some(name) = new_name.as_deref() {
        return set_hostname(name);
    }

    let name = get_hostname()?;
    let name = if short {
        shorten_hostname(&name)
    } else {
        name
    };
    let mut out = stdout();
    writeln!(out, "{name}").map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    Ok(())
}

fn shorten_hostname(name: &str) -> String {
    name.split('.').next().unwrap_or(name).to_owned()
}

fn get_hostname() -> Result<String, Vec<AppletError>> {
    let mut buf = vec![0u8; 256];
    // SAFETY: buf points to buf.len() writable bytes; gethostname writes a
    // NUL-terminated string into it on success.
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len() as _) };
    if ret != 0 {
        let e = std::io::Error::last_os_error();
        return Err(vec![AppletError::from_io(
            APPLET,
            "getting hostname",
            None,
            e,
        )]);
    }
    Ok(
        unsafe { CStr::from_ptr(buf.as_ptr() as *const libc::c_char) }
            .to_string_lossy()
            .into_owned(),
    )
}

fn set_hostname(name: &str) -> AppletResult {
    let c_name = CString::new(name)
        .map_err(|_| vec![AppletError::new(APPLET, "hostname contains NUL byte")])?;
    // SAFETY: c_name is a valid NUL-terminated C string.
    let ret = unsafe { libc::sethostname(c_name.as_ptr(), name.len() as _) };
    if ret != 0 {
        let e = std::io::Error::last_os_error();
        return Err(vec![AppletError::from_io(
            APPLET,
            "setting hostname",
            None,
            e,
        )]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{get_hostname, run, set_hostname, shorten_hostname};

    fn args(v: &[&str]) -> Vec<std::ffi::OsString> {
        v.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn current_hostname_is_non_empty() {
        assert!(!get_hostname().unwrap().is_empty());
    }

    #[test]
    fn file_option_requires_argument() {
        assert!(run(&args(&["-F"])).is_err());
    }

    #[test]
    fn hostname_rejects_nul_bytes() {
        assert!(set_hostname("bad\0name").is_err());
    }

    #[test]
    fn shortens_domain_name() {
        assert_eq!(shorten_hostname("host.example.com"), "host");
        assert_eq!(shorten_hostname("host"), "host");
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(run(&args(&["one", "two"])).is_err());
    }
}
