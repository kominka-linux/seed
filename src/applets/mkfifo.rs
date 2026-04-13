use std::ffi::CString;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;

const APPLET: &str = "mkfifo";
const DEFAULT_MODE: u32 = 0o666;

#[derive(Debug, Eq, PartialEq)]
struct Options {
    mode: u32,
    paths: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    let mut errors = Vec::new();

    for path in &options.paths {
        if let Err(error) = create_fifo(path, options.mode) {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut mode = DEFAULT_MODE;
    let mut paths = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if cursor.parsing_flags() && arg == "-m" {
            let value = cursor.next_value(APPLET, "m")?;
            mode = parse_mode(value)?;
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }

        paths.push(arg.to_owned());
    }

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(Options { mode, paths })
}

fn parse_mode(text: &str) -> Result<u32, Vec<AppletError>> {
    u32::from_str_radix(text, 8)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid mode '{text}'"))])
}

fn create_fifo(path: &str, mode: u32) -> Result<(), AppletError> {
    let path_c =
        CString::new(path).map_err(|_| AppletError::new(APPLET, "path contains NUL byte"))?;
    let mode =
        libc::mode_t::try_from(mode).map_err(|_| AppletError::new(APPLET, "mode out of range"))?;

    // SAFETY: `path_c` is a valid NUL-terminated C string and `mode` is a
    // plain value passed through to libc.
    let status = unsafe { libc::mkfifo(path_c.as_ptr(), mode) };
    if status == 0 {
        Ok(())
    } else {
        Err(AppletError::from_io(
            APPLET,
            "creating",
            Some(path),
            std::io::Error::last_os_error(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::FileTypeExt;

    use super::{DEFAULT_MODE, create_fifo, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_mode_and_paths() {
        let options = parse_args(&args(&["-m", "600", "first", "second"])).expect("parse mkfifo");
        assert_eq!(options.mode, 0o600);
        assert_eq!(options.paths, vec!["first", "second"]);
    }

    #[test]
    fn defaults_mode_when_omitted() {
        let options = parse_args(&args(&["pipe"])).expect("parse mkfifo");
        assert_eq!(options.mode, DEFAULT_MODE);
        assert_eq!(options.paths, vec!["pipe"]);
    }

    #[test]
    fn creates_fifo() {
        let dir = crate::common::unix::temp_dir("mkfifo-test");
        let path = dir.join("pipe");

        create_fifo(path.to_str().expect("utf-8 path"), 0o600).expect("create fifo");

        let metadata = fs::symlink_metadata(&path).expect("fifo metadata");
        assert!(metadata.file_type().is_fifo());

        fs::remove_dir_all(dir).ok();
    }
}
