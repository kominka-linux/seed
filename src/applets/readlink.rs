use std::fs;
use std::io::Write;
use std::path::Component;
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
        canonicalize_lenient(path)
    } else {
        fs::read_link(path)
    };

    resolved.map_err(|e| AppletError::from_io(APPLET, "reading", Some(path), e))
}

fn canonicalize_lenient(path: &str) -> std::io::Result<PathBuf> {
    let mut resolved = if std::path::Path::new(path).is_absolute() {
        PathBuf::from("/")
    } else {
        std::env::current_dir()?
    };
    let mut pending = path_components(path);
    let mut symlink_depth = 0_u8;

    while let Some(component) = pending.first().cloned() {
        pending.remove(0);
        match component {
            OwnedComponent::ParentDir => {
                resolved.pop();
            }
            OwnedComponent::Normal(name) => {
                let candidate = resolved.join(&name);
                match fs::symlink_metadata(&candidate) {
                    Ok(metadata) => {
                        if metadata.file_type().is_symlink() {
                            symlink_depth = symlink_depth.saturating_add(1);
                            if symlink_depth > 40 {
                                return Err(std::io::Error::from_raw_os_error(libc::ELOOP));
                            }
                            let target = fs::read_link(&candidate)?;
                            if target.is_absolute() {
                                resolved = PathBuf::from("/");
                            }
                            let target_parts = path_components(&target);
                            pending.splice(0..0, target_parts);
                        } else {
                            if !metadata.is_dir() && !pending.is_empty() {
                                return Err(std::io::Error::from_raw_os_error(libc::ENOTDIR));
                            }
                            resolved.push(name);
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        resolved.push(name);
                    }
                    Err(err) => return Err(err),
                }
            }
        }
    }

    Ok(resolved)
}

#[derive(Clone, Debug)]
enum OwnedComponent {
    ParentDir,
    Normal(std::ffi::OsString),
}

fn path_components(path: impl AsRef<std::path::Path>) -> Vec<OwnedComponent> {
    let mut parts = Vec::new();
    for component in path.as_ref().components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => parts.push(OwnedComponent::ParentDir),
            Component::Normal(name) => parts.push(OwnedComponent::Normal(name.to_os_string())),
            Component::Prefix(_) => {}
        }
    }
    parts
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

    #[test]
    fn canonicalizes_after_missing_parent_is_canceled_by_dotdot() {
        let dir = crate::common::unix::temp_dir("readlink-test");
        let target = dir.join("target");
        std::fs::write(&target, b"x").unwrap();

        let path = dir.join("missing").join("..").join("target");
        assert_eq!(
            resolve_path(path.to_str().unwrap(), true).unwrap(),
            target.canonicalize().unwrap()
        );

        std::fs::remove_dir_all(dir).ok();
    }

    use std::path::PathBuf;
}
