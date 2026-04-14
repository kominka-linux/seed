use std::fs;
use std::path::Path;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::extattr::{DIRSYNC, get_flags, parse_flag_mask, set_flags, set_version};

const APPLET: &str = "chattr";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Mode {
    #[default]
    Modify,
    Set,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    recursive: bool,
    add: u32,
    remove: u32,
    set: u32,
    mode: Mode,
    version: Option<u32>,
    operands: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    let mut errors = Vec::new();
    for operand in &options.operands {
        apply_to_path(Path::new(operand), &options, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-R" => {
                options.recursive = true;
                index += 1;
            }
            "-f" | "-V" => {
                index += 1;
            }
            "-v" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "v")]);
                };
                options.version = Some(parse_version(value)?);
                index += 2;
            }
            _ if is_flag_operation(arg) => {
                apply_flag_spec(&mut options, arg)?;
                index += 1;
            }
            _ => {
                options.operands.extend(args[index..].iter().cloned());
                break;
            }
        }
    }

    if options.mode == Mode::Set && (options.add != 0 || options.remove != 0) {
        return Err(vec![AppletError::new(APPLET, "= is incompatible with - and +")]);
    }
    if options.add & options.remove != 0 {
        return Err(vec![AppletError::new(APPLET, "can't set and unset a flag")]);
    }
    if options.mode == Mode::Modify
        && options.add == 0
        && options.remove == 0
        && options.version.is_none()
    {
        return Err(vec![AppletError::new(APPLET, "must use '-v', =, - or +")]);
    }
    if options.operands.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(options)
}

fn is_flag_operation(arg: &str) -> bool {
    matches!(arg.as_bytes().first(), Some(b'+') | Some(b'='))
        || (arg.starts_with('-') && arg.len() > 1 && arg != "-R" && arg != "-f" && arg != "-V" && arg != "-v")
}

fn apply_flag_spec(options: &mut Options, spec: &str) -> Result<(), Vec<AppletError>> {
    let op = spec.chars().next().expect("flag spec prefix");
    let mask = parse_flag_mask(&spec[1..]).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
    match op {
        '+' => options.add |= mask,
        '-' => options.remove |= mask,
        '=' => {
            options.mode = Mode::Set;
            options.set = mask;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn parse_version(value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .parse::<u32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid version '{value}'"))])
}

fn apply_to_path(path: &Path, options: &Options, errors: &mut Vec<AppletError>) {
    let path_text = path.to_string_lossy().into_owned();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            errors.push(AppletError::from_io(APPLET, "stat", Some(&path_text), err));
            return;
        }
    };

    if let Some(version) = options.version
        && let Err(err) = set_version(path, version)
    {
        errors.push(AppletError::from_io(
            APPLET,
            "setting version on",
            Some(&path_text),
            err,
        ));
    }

    let flags = match options.mode {
        Mode::Set => options.set,
        Mode::Modify => match get_flags(path) {
            Ok(flags) => (flags & !options.remove) | options.add,
            Err(err) => {
                errors.push(AppletError::from_io(
                    APPLET,
                    "reading flags on",
                    Some(&path_text),
                    err,
                ));
                return;
            }
        },
    };
    let flags = if metadata.is_dir() { flags } else { flags & !DIRSYNC };

    if let Err(err) = set_flags(path, flags) {
        errors.push(AppletError::from_io(
            APPLET,
            "setting flags on",
            Some(&path_text),
            err,
        ));
    }

    if options.recursive && metadata.is_dir() && !metadata.file_type().is_symlink() {
        let mut entries = match fs::read_dir(path) {
            Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
            Err(err) => {
                errors.push(AppletError::from_io(
                    APPLET,
                    "reading directory",
                    Some(&path_text),
                    err,
                ));
                return;
            }
        };
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            apply_to_path(&entry.path(), options, errors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, Options, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_recursive_version_and_flags() {
        assert_eq!(
            parse_args(&args(&["-R", "-v", "5", "+ia", "file"])).unwrap(),
            Options {
                recursive: true,
                add: 0x10 | 0x20,
                remove: 0,
                set: 0,
                mode: Mode::Modify,
                version: Some(5),
                operands: vec![String::from("file")],
            }
        );
    }

    #[test]
    fn rejects_mixed_set_and_add() {
        assert!(parse_args(&args(&["=i", "+a", "file"])).is_err());
    }
}
