use std::fs;
use std::path::Path;

use crate::common::applet::finish;
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::fs::move_path;

const APPLET: &str = "mv";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let (no_target_directory, sources, destination) = parse_args(args)?;
    let destination_path = Path::new(&destination);
    let dest_is_dir = !no_target_directory
        && fs::metadata(destination_path)
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

fn parse_args(args: &[String]) -> Result<(bool, Vec<String>, String), Vec<AppletError>> {
    let mut no_target_directory = false;
    let mut target_directory: Option<String> = None;
    let mut paths = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if cursor.parsing_flags() && arg == "--no-target-directory" {
            no_target_directory = true;
            continue;
        }
        if cursor.parsing_flags() && arg == "-t" {
            target_directory = Some(cursor.next_value(APPLET, "t")?.to_owned());
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'T' => no_target_directory = true,
                    'f' => {}
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.to_owned());
    }

    if let Some(directory) = target_directory {
        if paths.is_empty() {
            return Err(vec![AppletError::new(APPLET, "missing file operand")]);
        }
        return Ok((no_target_directory, paths, directory));
    }

    if paths.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let destination = paths.pop().expect("destination exists");
    Ok((no_target_directory, paths, destination))
}

fn file_name(path: &Path) -> &std::ffi::OsStr {
    path.file_name().unwrap_or(path.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn parse(input: &[&str]) -> (bool, Vec<String>, String) {
        let args = input.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        parse_args(&args).expect("parse args")
    }

    #[test]
    fn parses_no_target_directory() {
        let (no_target_directory, sources, destination) = parse(&["-T", "src", "dst"]);
        assert!(no_target_directory);
        assert_eq!(sources, vec!["src"]);
        assert_eq!(destination, "dst");

        let (no_target_directory, _, _) = parse(&["--no-target-directory", "src", "dst"]);
        assert!(no_target_directory);
    }
}
