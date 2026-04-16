use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "ln";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    symbolic: bool,
    force: bool,
    no_dereference: bool,
    no_target_directory: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let (options, sources, destination) = parse_args(&args)?;
    let destination_path = Path::new(&destination);
    let dest_is_dir = !options.no_target_directory
        && is_target_directory(destination_path, options.no_dereference);

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

        if let Err(err) = create_link(src_path, &target, options) {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>, String), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "--no-target-directory" {
            options.no_target_directory = true;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    's' => options.symbolic = true,
                    'f' => options.force = true,
                    'n' => options.no_dereference = true,
                    'T' => options.no_target_directory = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        paths.push(arg.clone());
    }

    if paths.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let destination = paths.pop().expect("destination exists");
    Ok((options, paths, destination))
}

fn is_target_directory(path: &Path, no_dereference: bool) -> bool {
    if no_dereference {
        fs::symlink_metadata(path)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    } else {
        fs::metadata(path)
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    }
}

fn create_link(source: &Path, target: &Path, options: Options) -> Result<(), AppletError> {
    if options.force {
        remove_existing_target(target)?;
    }

    if options.symbolic {
        symlink(source, target)
            .map_err(|e| AppletError::from_io(APPLET, "creating symlink", target.to_str(), e))
    } else {
        fs::hard_link(source, target)
            .map_err(|e| AppletError::from_io(APPLET, "creating link", target.to_str(), e))
    }
}

fn remove_existing_target(target: &Path) -> Result<(), AppletError> {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return Ok(());
    };

    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Err(AppletError::new(
            APPLET,
            format!("cannot overwrite directory '{}'", target.display()),
        ));
    }

    fs::remove_file(target)
        .map_err(|e| AppletError::from_io(APPLET, "removing", target.to_str(), e))
}

fn file_name(path: &Path) -> PathBuf {
    path.file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| path.as_os_str().into())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;

    use super::{Options, create_link, is_target_directory, parse_args};

    fn parse(input: &[&str]) -> (Options, Vec<String>, String) {
        let args = input.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        parse_args(&args).expect("parse args")
    }

    #[test]
    fn parses_symbolic_force_and_noderef() {
        let (options, sources, destination) = parse(&["-sfn", "src", "dst"]);
        assert!(options.symbolic);
        assert!(options.force);
        assert!(options.no_dereference);
        assert_eq!(sources, vec!["src"]);
        assert_eq!(destination, "dst");
    }

    #[test]
    fn parses_no_target_directory() {
        let (options, _, _) = parse(&["-T", "src", "dst"]);
        assert!(options.no_target_directory);

        let (options, _, _) = parse(&["--no-target-directory", "src", "dst"]);
        assert!(options.no_target_directory);
    }

    #[test]
    fn hard_link_creation_works() {
        let dir = crate::common::unix::temp_dir("ln-test");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::write(&src, b"hello").unwrap();

        create_link(&src, &dst, Options::default()).unwrap();

        assert_eq!(
            std::fs::metadata(&src).unwrap().ino(),
            std::fs::metadata(&dst).unwrap().ino()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn symbolic_link_creation_works() {
        let dir = crate::common::unix::temp_dir("ln-test");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::write(&src, b"hello").unwrap();

        create_link(
            &src,
            &dst,
            Options {
                symbolic: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(std::fs::read_link(&dst).unwrap(), src);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn force_replaces_existing_file() {
        let dir = crate::common::unix::temp_dir("ln-test");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::write(&src, b"hello").unwrap();
        std::fs::write(&dst, b"old").unwrap();

        create_link(
            &src,
            &dst,
            Options {
                force: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            std::fs::metadata(&src).unwrap().ino(),
            std::fs::metadata(&dst).unwrap().ino()
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_dereference_avoids_following_symlinked_directory() {
        let dir = crate::common::unix::temp_dir("ln-test");
        let target_dir = dir.join("target");
        let link_dir = dir.join("linkdir");
        std::fs::create_dir(&target_dir).unwrap();
        std::os::unix::fs::symlink(&target_dir, &link_dir).unwrap();

        assert!(!is_target_directory(&link_dir, true));
        assert!(is_target_directory(&link_dir, false));
        std::fs::remove_dir_all(dir).ok();
    }
}
