use std::fs;
use std::path::{Path, PathBuf};

use crate::common::applet::finish;
use crate::common::error::AppletError;

const APPLET: &str = "rmdir";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let (parents, paths) = parse_args(args)?;
    let mut errors = Vec::new();

    for path in &paths {
        let result = if parents {
            remove_with_parents(Path::new(path))
        } else {
            fs::remove_dir(path)
        };

        if let Err(err) = result {
            errors.push(AppletError::from_io(APPLET, "removing", Some(path), err));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(bool, Vec<String>), Vec<AppletError>> {
    let mut parents = false;
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
                    'p' => parents = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((parents, paths))
}

fn remove_with_parents(path: &Path) -> std::io::Result<()> {
    let mut current = PathBuf::from(path);
    loop {
        fs::remove_dir(&current)?;
        let Some(parent) = current.parent() else {
            return Ok(());
        };
        if parent.as_os_str().is_empty() {
            return Ok(());
        }
        current = parent.to_path_buf();
    }
}
