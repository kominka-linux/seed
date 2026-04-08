use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::common::error::AppletError;

const APPLET: &str = "mkdir";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
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

fn parse_args(args: &[String]) -> Result<(bool, Option<u32>, Vec<String>), Vec<AppletError>> {
    let mut parents = false;
    let mut mode = None;
    let mut paths = Vec::new();
    let mut parsing_flags = true;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }

        if parsing_flags && arg == "-m" {
            let Some(value) = args.get(index + 1) else {
                return Err(vec![AppletError::new(
                    APPLET,
                    "option requires an argument -- 'm'",
                )]);
            };
            mode = Some(parse_mode(value)?);
            index += 2;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'p' => parents = true,
                    _ => {
                        return Err(vec![AppletError::new(
                            APPLET,
                            format!("invalid option -- '{flag}'"),
                        )]);
                    }
                }
            }
            index += 1;
            continue;
        }

        paths.push(arg.clone());
        index += 1;
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
