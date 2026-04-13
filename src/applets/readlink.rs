use std::fs;
use std::io::Write;
use std::path::PathBuf;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "readlink";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (canonicalize, paths) = parse_args(args)?;
    let mut out = stdout();
    let mut errors = Vec::new();

    for path in &paths {
        match resolve_path(path, canonicalize) {
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

fn parse_args(args: &[String]) -> Result<(bool, Vec<String>), Vec<AppletError>> {
    let mut canonicalize = false;
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "-f" {
            canonicalize = true;
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

    Ok((canonicalize, paths))
}

fn resolve_path(path: &str, canonicalize: bool) -> Result<PathBuf, AppletError> {
    let resolved = if canonicalize {
        fs::canonicalize(path)
    } else {
        fs::read_link(path)
    };

    resolved.map_err(|e| AppletError::from_io(APPLET, "reading", Some(path), e))
}

#[cfg(test)]
mod tests {
    use super::{parse_args, resolve_path};

    #[test]
    fn parses_f_flag() {
        let args = vec!["-f".to_string(), "path".to_string()];
        let (canonicalize, paths) = parse_args(&args).unwrap();
        assert!(canonicalize);
        assert_eq!(paths, vec!["path"]);
    }

    #[test]
    fn reads_symlink_target() {
        let dir = crate::common::unix::temp_dir("readlink-test");
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink("target", &link).unwrap();

        assert_eq!(
            resolve_path(link.to_str().unwrap(), false).unwrap(),
            PathBuf::from("target")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn canonicalizes_existing_path() {
        let dir = crate::common::unix::temp_dir("readlink-test");
        let target = dir.join("target");
        let link = dir.join("link");
        std::fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            resolve_path(link.to_str().unwrap(), true).unwrap(),
            target.canonicalize().unwrap()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    use std::path::PathBuf;
}
