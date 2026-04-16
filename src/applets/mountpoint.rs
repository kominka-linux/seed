use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::common::applet::finish_code;
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::mounts;

const APPLET: &str = "mountpoint";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    quiet: bool,
    fs_devno: bool,
    nofollow: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let (options, path) = parse_args(args)?;
    run_linux(options, &path)
}

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

fn parse_args(args: &[std::ffi::OsString]) -> Result<(Options, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut path = None;
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('q') => options.quiet = true,
            ParsedArg::Short('d') => options.fs_devno = true,
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) if name == "quiet" => options.quiet = true,
            ParsedArg::Long(name) if name == "fs-devno" => options.fs_devno = true,
            ParsedArg::Long(name) if name == "nofollow" => options.nofollow = true,
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
                if path.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                path = Some(arg);
            }
        }
    }

    let path = path.ok_or_else(|| vec![AppletError::new(APPLET, "missing operand")])?;
    Ok((options, path))
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
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
