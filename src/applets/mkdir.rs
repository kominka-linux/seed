use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::common::applet::finish;
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;

const APPLET: &str = "mkdir";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let (parents, mode, paths) = parse_args(args)?;
    let mut errors = Vec::new();

    for path in &paths {
        let create_result = if parents {
            fs::create_dir_all(path)
        } else {
            fs::create_dir(path)
        };

        match create_result {
            Ok(()) => {
                if let Some(mode) = mode
                    && let Err(err) = fs::set_permissions(path, fs::Permissions::from_mode(mode))
                {
                    errors.push(AppletError::from_io(
                        APPLET,
                        "setting mode on",
                        Some(path),
                        err,
                    ));
                }
            }
            Err(err) => errors.push(AppletError::from_io(APPLET, "creating", Some(path), err)),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(bool, Option<u32>, Vec<String>), Vec<AppletError>> {
    let mut parents = false;
    let mut mode = None;
    let mut paths = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        if cursor.parsing_flags() && arg == "-m" {
            let value = cursor.next_value(APPLET, "m")?;
            mode = Some(parse_mode(value)?);
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'p' => parents = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.to_owned());
    }

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((parents, mode, paths))
}

fn parse_mode(mode: &str) -> Result<u32, Vec<AppletError>> {
    u32::from_str_radix(mode, 8)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid mode '{mode}'"))])
}

#[allow(dead_code)]
fn _path_exists(path: &Path) -> bool {
    path.exists()
}
