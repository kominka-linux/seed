use std::fs;
use std::path::Path;

use crate::common::error::AppletError;
use crate::common::fs::move_path;

const APPLET: &str = "mv";

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
    let (sources, destination) = parse_args(args)?;
    let destination_path = Path::new(&destination);
    let dest_is_dir = fs::metadata(destination_path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false);

    if sources.len() > 1 && !dest_is_dir {
        return Err(vec![AppletError::new(
            APPLET,
            format!("target '{destination}' is not a directory"),
        )]);
    }

    let mut errors = Vec::new();
    for source in &sources {
        let src_path = Path::new(source);
        let target = if dest_is_dir {
            destination_path.join(file_name(src_path))
        } else {
            destination_path.to_path_buf()
        };

        if let Err(err) = move_path(src_path, &target) {
            errors.push(AppletError::from_io(APPLET, "moving", Some(source), err));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Vec<String>, String), Vec<AppletError>> {
    let mut parsing_flags = true;
    let mut target_directory: Option<String> = None;
    let mut paths = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }

        if parsing_flags && arg == "-t" {
            let Some(directory) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "t")]);
            };
            target_directory = Some(directory.clone());
            index += 2;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'f' => {}
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            index += 1;
            continue;
        }

        paths.push(arg.clone());
        index += 1;
    }

    if let Some(directory) = target_directory {
        if paths.is_empty() {
            return Err(vec![AppletError::new(APPLET, "missing file operand")]);
        }
        return Ok((paths, directory));
    }

    if paths.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let destination = paths.pop().expect("destination exists");
    Ok((paths, destination))
}

fn file_name(path: &Path) -> &std::ffi::OsStr {
    path.file_name().unwrap_or(path.as_os_str())
}
