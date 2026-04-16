use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;

const APPLET: &str = "touch";

#[derive(Clone, Copy, Debug, Default)]
struct Options<'a> {
    no_create: bool,
    access_only: bool,
    modify_only: bool,
    reference: Option<&'a str>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let (options, paths) = parse_args(args)?;
    let mut errors = Vec::new();

    for path in &paths {
        if let Err(err) = touch_path(path, options) {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options<'_>, Vec<&str>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-c" | "--no-create" => options.no_create = true,
            "-a" => options.access_only = true,
            "-m" => options.modify_only = true,
            "-r" | "--reference" => {
                options.reference = Some(cursor.next_value(APPLET, "r")?);
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => paths.push(arg),
        }
    }

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    Ok((options, paths))
}

fn touch_path(path: &str, options: Options<'_>) -> Result<(), AppletError> {
    let target = Path::new(path);
    if !target.exists() {
        if options.no_create {
            return Ok(());
        }
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(target)
            .map_err(|e| AppletError::from_io(APPLET, "opening", Some(path), e))?;
    }

    let times = if let Some(reference) = options.reference {
        reference_times(reference, options)?
    } else {
        current_times(options)
    };

    set_times(target, &times).map_err(|e| AppletError::from_io(APPLET, "touching", Some(path), e))
}

fn current_times(options: Options<'_>) -> [libc::timespec; 2] {
    [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: if options.modify_only {
                libc::UTIME_OMIT
            } else {
                libc::UTIME_NOW
            },
        },
        libc::timespec {
            tv_sec: 0,
            tv_nsec: if options.access_only {
                libc::UTIME_OMIT
            } else {
                libc::UTIME_NOW
            },
        },
    ]
}

fn reference_times(
    reference: &str,
    options: Options<'_>,
) -> Result<[libc::timespec; 2], AppletError> {
    let metadata = fs::metadata(reference)
        .map_err(|e| AppletError::from_io(APPLET, "reading", Some(reference), e))?;
    Ok([
        libc::timespec {
            tv_sec: metadata.atime(),
            tv_nsec: if options.modify_only {
                libc::UTIME_OMIT
            } else {
                metadata.atime_nsec()
            },
        },
        libc::timespec {
            tv_sec: metadata.mtime(),
            tv_nsec: if options.access_only {
                libc::UTIME_OMIT
            } else {
                metadata.mtime_nsec()
            },
        },
    ])
}

fn set_times(path: &Path, times: &[libc::timespec; 2]) -> std::io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL byte")
    })?;
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;
    use std::ffi::OsString;
    use std::thread;
    use std::time::Duration;

    use super::{Options, parse_args, touch_path};

    fn parse(input: &[&str]) -> (bool, bool, bool, Option<String>, Vec<String>) {
        let args = input
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (options, paths) = parse_args(&args).expect("parse args");
        (
            options.no_create,
            options.access_only,
            options.modify_only,
            options.reference.map(str::to_string),
            paths.into_iter().map(str::to_string).collect(),
        )
    }

    #[test]
    fn parses_reference_and_flags() {
        let (no_create, access_only, modify_only, reference, paths) =
            parse(&["-c", "-a", "-r", "ref", "file"]);
        assert!(no_create);
        assert!(access_only);
        assert!(!modify_only);
        assert_eq!(reference.as_deref(), Some("ref"));
        assert_eq!(paths, vec!["file"]);
    }

    #[test]
    fn creates_missing_file_by_default() {
        let dir = crate::common::unix::temp_dir("touch-test");
        let path = dir.join("file");
        touch_path(path.to_str().unwrap(), Options::default()).unwrap();
        assert!(path.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_create_skips_missing_file() {
        let dir = crate::common::unix::temp_dir("touch-test");
        let path = dir.join("missing");
        touch_path(
            path.to_str().unwrap(),
            Options {
                no_create: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reference_copies_mtime() {
        let dir = crate::common::unix::temp_dir("touch-test");
        let source = dir.join("source");
        let target = dir.join("target");
        std::fs::write(&source, b"a").unwrap();
        thread::sleep(Duration::from_secs(1));
        std::fs::write(&target, b"b").unwrap();

        touch_path(
            target.to_str().unwrap(),
            Options {
                reference: Some(source.to_str().unwrap()),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::metadata(&target).unwrap().mtime(),
            std::fs::metadata(&source).unwrap().mtime()
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
