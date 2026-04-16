use std::path::Path;

use crate::common::applet::finish;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::fs::remove_path;

const APPLET: &str = "rm";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    force: bool,
    recursive: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let (options, paths) = parse_args(&args)?;
    let mut errors = Vec::new();

    for path in &paths {
        if let Err(err) = remove_path(Path::new(path), options.recursive, options.force) {
            errors.push(AppletError::from_io(APPLET, "removing", Some(path), err));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'f' => options.force = true,
                    'r' | 'R' => options.recursive = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    if paths.is_empty() && !options.force {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((options, paths))
}
