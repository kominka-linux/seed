use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::fs::{CopyContext, CopyError, CopyOptions, Dereference, copy_path};

const APPLET: &str = "cp";

#[derive(Clone, Copy, Debug)]
struct Options {
    recursive: bool,
    dereference: Dereference,
    preserve_hard_links: bool,
    preserve_mode: bool,
    preserve_timestamps: bool,
    parents: bool,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, sources, destination) = parse_args(args)?;
    let destination_path = Path::new(&destination);
    let dest_is_dir = fs::metadata(destination_path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false);

    if sources.len() > 1 && !dest_is_dir {
        return Err(vec![AppletError::new(
            APPLET,
            format!("target '{destination}' is not a directory"),
        )]);
    }

    let copy_options = CopyOptions {
        recursive: options.recursive,
        dereference: options.dereference,
        preserve_hard_links: options.preserve_hard_links,
        preserve_mode: options.preserve_mode,
        preserve_timestamps: options.preserve_timestamps,
    };
    let mut context = CopyContext::new();
    let mut errors = Vec::new();

    for source in &sources {
        let src_path = Path::new(source);
        let target =
            destination_for_source(src_path, destination_path, dest_is_dir, options.parents);
        if same_file(src_path, &target, options.dereference) {
            errors.push(AppletError::new(
                APPLET,
                format!(
                    "'{}' and '{}' are the same file",
                    src_path.display(),
                    target.display()
                ),
            ));
            continue;
        }
        match copy_path(src_path, &target, &copy_options, &mut context, true) {
            Ok(()) => {}
            Err(CopyError::OmitDirectory) => {
                errors.push(AppletError::new(
                    APPLET,
                    format!("omitting directory '{source}'"),
                ));
            }
            Err(CopyError::Io(err)) => {
                errors.push(AppletError::from_io(APPLET, "copying", Some(source), err))
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>, String), Vec<AppletError>> {
    let mut recursive = false;
    let mut preserve_hard_links = false;
    let mut preserve_mode = false;
    let mut preserve_timestamps = false;
    let mut parents = false;
    let mut want_never = false;
    let mut want_command_line = false;
    let mut want_always = false;
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg == "--parents" {
            parents = true;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'a' => {
                        recursive = true;
                        preserve_hard_links = true;
                        preserve_mode = true;
                        preserve_timestamps = true;
                        want_never = true;
                    }
                    'd' | 'P' => {
                        preserve_hard_links = true;
                        want_never = true;
                    }
                    'H' => want_command_line = true,
                    'L' => want_always = true,
                    'R' | 'r' => recursive = true,
                    'p' => {
                        preserve_mode = true;
                        preserve_timestamps = true;
                    }
                    'f' => {}
                    _ => {
                        return Err(vec![AppletError::invalid_option(APPLET, flag)]);
                    }
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
    let dereference = if want_always {
        Dereference::Always
    } else if want_command_line {
        Dereference::CommandLine
    } else if want_never || recursive {
        Dereference::Never
    } else {
        Dereference::Always
    };

    Ok((
        Options {
            recursive,
            dereference,
            preserve_hard_links,
            preserve_mode,
            preserve_timestamps,
            parents,
        },
        paths,
        destination,
    ))
}

fn destination_for_source(src: &Path, dest: &Path, dest_is_dir: bool, parents: bool) -> PathBuf {
    if parents {
        return dest.join(strip_root(src));
    }

    if dest_is_dir {
        return dest.join(file_name(src));
    }

    dest.to_path_buf()
}

fn strip_root(path: &Path) -> PathBuf {
    let mut stripped = PathBuf::new();
    for component in path.components() {
        if let std::path::Component::Normal(part) = component {
            stripped.push(part);
        }
    }
    stripped
}

fn file_name(path: &Path) -> &std::ffi::OsStr {
    path.file_name().unwrap_or(path.as_os_str())
}

fn same_file(source: &Path, target: &Path, dereference: Dereference) -> bool {
    let source_metadata = metadata_for_compare(source, dereference);
    let target_metadata = metadata_for_compare(target, dereference);
    match (source_metadata, target_metadata) {
        (Ok(source), Ok(target)) => source.dev() == target.dev() && source.ino() == target.ino(),
        _ => false,
    }
}

fn metadata_for_compare(path: &Path, dereference: Dereference) -> std::io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() && dereference != Dereference::Never {
        fs::metadata(path)
    } else {
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{Options, metadata_for_compare, parse_args, same_file};
    use crate::common::fs::Dereference;

    fn parse(input: &[&str]) -> (Options, Vec<String>, String) {
        let args = input.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        parse_args(&args).expect("parse args")
    }

    #[test]
    fn recursive_copy_preserves_symlinks_by_default() {
        let (options, _, _) = parse(&["-R", "src", "dest"]);
        assert!(options.recursive);
        assert_eq!(options.dereference, Dereference::Never);
    }

    #[test]
    fn l_overrides_h_and_p() {
        let (options, _, _) = parse(&["-RHP", "-L", "src", "dest"]);
        assert_eq!(options.dereference, Dereference::Always);
    }

    #[test]
    fn detects_same_regular_file() {
        let dir = crate::common::unix::temp_dir("cp-same-file");
        let path = dir.join("file");
        fs::write(&path, b"hello").expect("write file");
        assert!(same_file(&path, &path, Dereference::Always));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dereference_mode_controls_symlink_comparison() {
        let dir = crate::common::unix::temp_dir("cp-same-symlink");
        let path = dir.join("file");
        let link = dir.join("link");
        fs::write(&path, b"hello").expect("write file");
        symlink(&path, &link).expect("create symlink");

        assert!(!same_file(&link, &path, Dereference::Never));
        assert!(same_file(&link, &path, Dereference::Always));
        assert!(metadata_for_compare(&link, Dereference::Always).is_ok());

        let _ = fs::remove_dir_all(dir);
    }
}
