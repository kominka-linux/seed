#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
#[cfg(target_os = "linux")]
use std::path::Path;

use crate::common::error::AppletError;
#[cfg(target_os = "linux")]
use crate::common::mounts;

const APPLET: &str = "mountpoint";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    quiet: bool,
    fs_devno: bool,
    nofollow: bool,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let (options, path) = parse_args(args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(options, &path);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = (options, path);

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

#[cfg(target_os = "linux")]
fn run_linux(options: Options, path: &str) -> Result<i32, Vec<AppletError>> {
    let mount = mounts::mountpoint_at_path(Path::new(path), options.nofollow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;

    if mount.is_some() {
        if options.fs_devno {
            let metadata = if options.nofollow {
                fs::symlink_metadata(path)
            } else {
                fs::metadata(path)
            }
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
            println!("{}", mounts::format_device_number(metadata.dev()));
        } else if !options.quiet {
            println!("{path} is a mountpoint");
        }
        Ok(0)
    } else {
        if !options.quiet {
            println!("{path} is not a mountpoint");
        }
        Ok(32)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut path = None;
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags {
            match arg.as_str() {
                "-q" | "--quiet" => {
                    options.quiet = true;
                    continue;
                }
                "-d" | "--fs-devno" => {
                    options.fs_devno = true;
                    continue;
                }
                "--nofollow" => {
                    options.nofollow = true;
                    continue;
                }
                _ if arg.starts_with('-') => {
                    return Err(vec![AppletError::invalid_option(
                        APPLET,
                        arg.chars().nth(1).unwrap_or('-'),
                    )]);
                }
                _ => {}
            }
        }

        if path.is_some() {
            return Err(vec![AppletError::new(APPLET, "extra operand")]);
        }
        path = Some(arg.clone());
    }

    let path = path.ok_or_else(|| vec![AppletError::new(APPLET, "missing operand")])?;
    Ok((options, path))
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_path() {
        let (options, path) =
            parse_args(&args(&["-q", "--nofollow", "-d", "/proc"])).expect("parse mountpoint");
        assert_eq!(
            options,
            Options {
                quiet: true,
                fs_devno: true,
                nofollow: true,
            }
        );
        assert_eq!(path, "/proc");
    }

    #[test]
    fn rejects_missing_path() {
        assert!(parse_args(&args(&["-q"])).is_err());
    }
}
