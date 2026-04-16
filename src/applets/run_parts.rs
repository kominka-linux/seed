use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "run-parts";
const EXIT_CANNOT_INVOKE: i32 = 126;
const EXIT_NOT_FOUND: i32 = 127;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    test_only: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let (options, directory) = parse_args(&args)?;
    let entries = collect_runnable_entries(&directory)?;

    if options.test_only {
        let mut out = stdout();
        for entry in entries {
            use std::io::Write;
            writeln!(out, "{}", entry.display())
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
        return Ok(0);
    }

    for entry in entries {
        let status = Command::new(&entry).status().map_err(|err| {
            let code = match err.kind() {
                std::io::ErrorKind::NotFound => EXIT_NOT_FOUND,
                _ => EXIT_CANNOT_INVOKE,
            };
            vec![AppletError::new(
                APPLET,
                format!(
                    "failed to execute '{}' (exit {code}): {err}",
                    entry.display()
                ),
            )]
        })?;
        let code = exit_code(status);
        if code != 0 {
            return Ok(code);
        }
    }

    Ok(0)
}

fn parse_args(args: &[String]) -> Result<(Options, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut parsing_flags = true;
    let mut paths = Vec::new();

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg == "--test" {
            options.test_only = true;
            continue;
        }

        if parsing_flags && arg.starts_with("--") {
            return Err(vec![AppletError::unrecognized_option(APPLET, arg)]);
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }

        paths.push(arg.clone());
    }

    match paths.as_slice() {
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        [directory] => Ok((options, directory.clone())),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn collect_runnable_entries(directory: &str) -> Result<Vec<PathBuf>, Vec<AppletError>> {
    let mut entries = Vec::new();
    let dir = fs::read_dir(directory).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            Some(directory),
            err,
        )]
    })?;

    for entry in dir {
        let entry =
            entry.map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            entries.push(path);
        }
    }

    entries.sort();
    Ok(entries)
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    128 + status.signal().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use crate::common::unix::temp_dir;

    use super::{Options, collect_runnable_entries, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_test_mode() {
        let (options, directory) =
            parse_args(&args(&["--test", "scripts"])).expect("parse run-parts");
        assert_eq!(options, Options { test_only: true });
        assert_eq!(directory, "scripts");
    }

    #[test]
    fn collect_runnable_entries_skips_non_executable_files() {
        let dir = temp_dir("run-parts");
        let first = dir.join("20-second");
        let second = dir.join("10-first");
        let skipped = dir.join("30-skip");

        fs::write(&first, "#!/bin/sh\n").expect("write first");
        fs::write(&second, "#!/bin/sh\n").expect("write second");
        fs::write(&skipped, "#!/bin/sh\n").expect("write skipped");

        let mut mode = fs::metadata(&first).expect("metadata first").permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&first, mode).expect("chmod first");

        let mut mode = fs::metadata(&second)
            .expect("metadata second")
            .permissions();
        mode.set_mode(0o755);
        fs::set_permissions(&second, mode).expect("chmod second");

        let entries =
            collect_runnable_entries(dir.to_str().expect("dir utf8")).expect("collect entries");
        assert_eq!(entries, vec![second, first]);

        fs::remove_dir_all(dir).expect("remove temp dir");
    }
}
