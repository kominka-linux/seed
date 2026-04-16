use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "realpath";

pub fn main(args: &[OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let paths = parse_args(&args)?;
    let mut out = stdout();
    let mut errors = Vec::new();

    for path in &paths {
        match canonicalize(path) {
            Ok(resolved) => {
                if let Err(e) = writeln!(out, "{}", resolved.display()) {
                    errors.push(AppletError::from_io(APPLET, "writing", None, e));
                }
            }
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<Vec<String>, Vec<AppletError>> {
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        paths.push(arg.clone());
    }

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(paths)
}

fn canonicalize(path: &str) -> Result<PathBuf, AppletError> {
    fs::canonicalize(path).map_err(|e| AppletError::from_io(APPLET, "reading", Some(path), e))
}

#[cfg(test)]
mod tests {
    use super::{canonicalize, parse_args};

    #[test]
    fn requires_operand() {
        assert!(parse_args(&[]).is_err());
    }

    #[test]
    fn canonicalizes_existing_path() {
        let dir = crate::common::unix::temp_dir("realpath-test");
        let target = dir.join("target");
        std::fs::write(&target, b"x").unwrap();

        assert_eq!(
            canonicalize(target.to_str().unwrap()).unwrap(),
            target.canonicalize().unwrap()
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
