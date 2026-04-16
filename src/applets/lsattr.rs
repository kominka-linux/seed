use std::fs;
use std::io::Write;
use std::path::Path;

use crate::common::applet::finish;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::extattr::{format_long_flags, format_short_flags, get_flags, get_version};
use crate::common::io::stdout;

const APPLET: &str = "lsattr";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    recursive: bool,
    all: bool,
    directories_only: bool,
    long_names: bool,
    show_version: bool,
    operands: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let mut options = parse_args(&args)?;
    if options.operands.is_empty() {
        options.operands.push(String::from("."));
    }

    let mut out = stdout();
    let mut errors = Vec::new();
    for operand in &options.operands {
        list_argument(Path::new(operand), &options, &mut out, &mut errors);
    }
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'R' => options.recursive = true,
                    'a' => options.all = true,
                    'd' => options.directories_only = true,
                    'l' => options.long_names = true,
                    'v' => options.show_version = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
        } else {
            options.operands.push(arg.clone());
        }
    }

    Ok(options)
}

fn list_argument(
    path: &Path,
    options: &Options,
    out: &mut impl Write,
    errors: &mut Vec<AppletError>,
) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) => {
            errors.push(AppletError::from_io(
                APPLET,
                "stat",
                Some(&path.display().to_string()),
                err,
            ));
            return;
        }
    };

    if metadata.is_dir() && !options.directories_only {
        list_directory(path, options, out, errors);
    } else {
        print_entry(path, options, out, errors);
    }
}

fn list_directory(
    directory: &Path,
    options: &Options,
    out: &mut impl Write,
    errors: &mut Vec<AppletError>,
) {
    let mut entries = match fs::read_dir(directory) {
        Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(err) => {
            errors.push(AppletError::from_io(
                APPLET,
                "reading directory",
                Some(&directory.display().to_string()),
                err,
            ));
            return;
        }
    };
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let name = entry.file_name();
        if !options.all && name.to_string_lossy().starts_with('.') {
            continue;
        }

        let path = entry.path();
        print_entry(&path, options, out, errors);
        if !options.recursive {
            continue;
        }

        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if writeln!(out, "\n{}:", path.display()).is_err() {
                errors.push(AppletError::new(APPLET, "writing stdout failed"));
                return;
            }
            list_directory(&path, options, out, errors);
            if writeln!(out).is_err() {
                errors.push(AppletError::new(APPLET, "writing stdout failed"));
                return;
            }
        }
    }
}

fn print_entry(
    path: &Path,
    options: &Options,
    out: &mut impl Write,
    errors: &mut Vec<AppletError>,
) {
    match format_entry(path, options) {
        Ok(line) => {
            if let Err(err) = writeln!(out, "{line}") {
                errors.push(AppletError::from_io(APPLET, "writing stdout", None, err));
            }
        }
        Err(err) => errors.push(err),
    }
}

fn format_entry(path: &Path, options: &Options) -> Result<String, AppletError> {
    let path_text = path.to_string_lossy().into_owned();
    let flags = get_flags(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading flags on", Some(&path_text), err))?;
    let prefix = if options.show_version {
        let version = get_version(path).map_err(|err| {
            AppletError::from_io(APPLET, "reading version on", Some(&path_text), err)
        })?;
        format!("{version:>5} ")
    } else {
        String::new()
    };

    if options.long_names {
        Ok(format!(
            "{prefix}{path_text:<28} {}",
            format_long_flags(flags)
        ))
    } else {
        Ok(format!("{prefix}{} {path_text}", format_short_flags(flags)))
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, format_entry, parse_args};
    use crate::common::test_env;
    use std::env;
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_combined_flags() {
        assert_eq!(
            parse_args(&args(&["-Rdlv", "file"])).unwrap(),
            Options {
                recursive: true,
                all: false,
                directories_only: true,
                long_names: true,
                show_version: true,
                operands: vec![String::from("file")],
            }
        );
    }

    #[test]
    fn formats_entry_from_state_backend() {
        let _lock = test_env::lock();
        let tmpdir = env::temp_dir().join(format!("seed-lsattr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmpdir);
        fs::create_dir_all(&tmpdir).expect("create temp dir");
        let state = tmpdir.join("attrs.tsv");
        let file = tmpdir.join("file");
        fs::write(&file, "x").expect("write file");
        fs::write(&state, format!("{}\t48\t7\n", file.display())).expect("write state");
        // SAFETY: the test holds the global env lock for the full mutation window.
        unsafe {
            env::set_var("SEED_EXTATTR_STATE", &state);
        }
        let line = format_entry(
            &file,
            &Options {
                show_version: true,
                ..Options::default()
            },
        )
        .expect("format entry");
        // SAFETY: the test holds the global env lock for the full mutation window.
        unsafe {
            env::remove_var("SEED_EXTATTR_STATE");
        }
        assert_eq!(line, format!("    7 -----ia------ {}", file.display()));
        let _ = fs::remove_dir_all(tmpdir);
    }
}
