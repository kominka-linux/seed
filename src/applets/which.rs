use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "which";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(found_all) => i32::from(!found_all),
        Err(errors) => {
            for e in errors {
                e.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<bool, Vec<AppletError>> {
    let mut all = false;
    let mut commands: Vec<&str> = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "-a" {
            all = true;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        commands.push(arg);
    }

    if commands.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let path_dirs = path_dirs();
    let mut out = stdout();
    let mut found_all = true;

    for cmd in &commands {
        // A command containing '/' is looked up directly, not via PATH.
        if cmd.contains('/') {
            if is_executable(Path::new(cmd)) {
                writeln!(out, "{cmd}")
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            } else {
                found_all = false;
            }
            continue;
        }

        let mut found = false;
        for dir in &path_dirs {
            let path = dir.join(cmd);
            if is_executable(&path) {
                writeln!(out, "{}", path.display())
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
                found = true;
                if !all {
                    break;
                }
            }
        }
        if !found {
            found_all = false;
        }
    }

    Ok(found_all)
}

fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default()
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use super::is_executable;

    #[test]
    fn executable_file_detected() {
        let dir = crate::common::unix::temp_dir("which-test");
        let path = dir.join("prog");
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&path));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn non_executable_file_not_detected() {
        let dir = crate::common::unix::temp_dir("which-test");
        let path = dir.join("data");
        std::fs::write(&path, b"hello").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&path));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn directory_not_detected_as_executable() {
        let dir = crate::common::unix::temp_dir("which-test");
        assert!(!is_executable(&dir));
        std::fs::remove_dir_all(dir).ok();
    }
}
