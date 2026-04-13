use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "install";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    create_parents: bool,
    create_directories: bool,
    mode: Option<u32>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, paths) = parse_args(args)?;

    if options.create_directories {
        return install_directories(&paths, options.mode);
    }

    let (source, destination) = match paths.as_slice() {
        [source, destination] => (source, destination),
        [] | [_] => {
            return Err(vec![AppletError::new(APPLET, "missing file operand")]);
        }
        _ => {
            return Err(vec![AppletError::new(APPLET, "extra operand")]);
        }
    };

    install_file(source, destination, options)
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut index = 0;
    let mut parsing_flags = true;

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-D" {
            options.create_parents = true;
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-d" {
            options.create_directories = true;
            index += 1;
            continue;
        }
        if parsing_flags && arg == "-m" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "m")]);
            };
            options.mode = Some(parse_mode(value)?);
            index += 1;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        paths.push(arg.clone());
        index += 1;
    }

    Ok((options, paths))
}

fn parse_mode(text: &str) -> Result<u32, Vec<AppletError>> {
    u32::from_str_radix(text, 8)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid mode '{text}'"))])
}

fn install_directories(paths: &[String], mode: Option<u32>) -> AppletResult {
    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let mut errors = Vec::new();
    for path in paths {
        if let Err(err) = fs::create_dir_all(path) {
            errors.push(AppletError::from_io(APPLET, "creating", Some(path), err));
            continue;
        }
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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn install_file(source: &str, destination: &str, options: Options) -> AppletResult {
    let source_path = Path::new(source);
    let destination_path = Path::new(destination);
    let target = if destination_path.is_dir() {
        destination_path.join(file_name(source_path))
    } else {
        destination_path.to_path_buf()
    };

    if options.create_parents {
        ensure_parent(&target).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "creating",
                Some(destination),
                err,
            )]
        })?;
    }

    fs::copy(source_path, &target)
        .map_err(|err| vec![AppletError::from_io(APPLET, "copying", Some(source), err)])?;

    if let Some(mode) = options.mode {
        fs::set_permissions(&target, fs::Permissions::from_mode(mode)).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "setting mode on",
                Some(&target.display().to_string()),
                err,
            )]
        })?;
    }

    Ok(())
}

fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn file_name(path: &Path) -> &std::ffi::OsStr {
    path.file_name().unwrap_or(path.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_directory_and_mode_flags() {
        let (options, paths) =
            parse_args(&args(&["-d", "-m", "755", "dir"])).expect("parse install");
        assert_eq!(
            options,
            Options {
                create_parents: false,
                create_directories: true,
                mode: Some(0o755),
            }
        );
        assert_eq!(paths, vec!["dir"]);
    }
}
