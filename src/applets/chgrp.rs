use std::ffi::CString;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "chgrp";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (group, paths) = parse_args(&args)?;
    let mut errors = Vec::new();

    for path in &paths {
        let path_c = match CString::new(path.as_str()) {
            Ok(path_c) => path_c,
            Err(_) => {
                errors.push(AppletError::new(APPLET, "path contains NUL byte"));
                continue;
            }
        };
        // SAFETY: `path_c` is a valid NUL-terminated path string and ids are
        // plain values passed through to libc.
        let rc = unsafe { libc::chown(path_c.as_ptr(), !0 as libc::uid_t, group) };
        if rc != 0 {
            errors.push(AppletError::from_io(
                APPLET,
                "changing group of",
                Some(path),
                std::io::Error::last_os_error(),
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(libc::gid_t, Vec<String>), Vec<AppletError>> {
    let Some(spec) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    if args.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let group = parse_group(spec)?;
    Ok((group, args[1..].to_vec()))
}

fn parse_group(text: &str) -> Result<libc::gid_t, Vec<AppletError>> {
    if let Ok(id) = text.parse::<libc::gid_t>() {
        return Ok(id);
    }
    let name = CString::new(text)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid group '{text}'"))])?;
    // SAFETY: `name` is a valid NUL-terminated C string.
    let entry = unsafe { libc::getgrnam(name.as_ptr()) };
    if entry.is_null() {
        Err(vec![AppletError::new(
            APPLET,
            format!("unknown group '{text}'"),
        )])
    } else {
        // SAFETY: non-null pointer returned by libc for the duration of this call.
        Ok(unsafe { (*entry).gr_gid })
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_numeric_group() {
        let (group, paths) = parse_args(&args(&["456", "file"])).expect("parse chgrp");
        assert_eq!(group, 456);
        assert_eq!(paths, vec!["file"]);
    }
}
