use std::ffi::CString;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "chown";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (owner, group, paths) = parse_args(&args)?;
    let mut errors = Vec::new();

    for path in &paths {
        let path_c = match CString::new(path.as_str()) {
            Ok(path_c) => path_c,
            Err(_) => {
                errors.push(AppletError::new(APPLET, "path contains NUL byte"));
                continue;
            }
        };
        // SAFETY: `path_c` is a valid NUL-terminated path string and uid/gid are
        // plain values passed through to libc.
        let rc = unsafe { libc::chown(path_c.as_ptr(), owner, group) };
        if rc != 0 {
            errors.push(AppletError::from_io(
                APPLET,
                "changing ownership of",
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

fn parse_args(
    args: &[String],
) -> Result<(libc::uid_t, libc::gid_t, Vec<String>), Vec<AppletError>> {
    let Some(spec) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    if args.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let (owner_text, group_text) = spec
        .split_once(':')
        .map_or((spec.as_str(), None), |(owner, group)| (owner, Some(group)));

    let owner = parse_user(owner_text)?;
    let group = match group_text {
        Some(group) => parse_group(group)?,
        None => !0 as libc::gid_t,
    };

    Ok((owner, group, args[1..].to_vec()))
}

fn parse_user(text: &str) -> Result<libc::uid_t, Vec<AppletError>> {
    if let Ok(id) = text.parse::<libc::uid_t>() {
        return Ok(id);
    }
    let name = CString::new(text)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid user '{text}'"))])?;
    // SAFETY: `name` is a valid NUL-terminated C string.
    let entry = unsafe { libc::getpwnam(name.as_ptr()) };
    if entry.is_null() {
        Err(vec![AppletError::new(
            APPLET,
            format!("unknown user '{text}'"),
        )])
    } else {
        // SAFETY: non-null pointer returned by libc for the duration of this call.
        Ok(unsafe { (*entry).pw_uid })
    }
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
    fn parses_numeric_owner_and_group() {
        let (owner, group, paths) = parse_args(&args(&["123:456", "file"])).expect("parse chown");
        assert_eq!(owner, 123);
        assert_eq!(group, 456);
        assert_eq!(paths, vec!["file"]);
    }
}
