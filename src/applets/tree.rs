use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "tree";

#[derive(Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    metadata: fs::Metadata,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let paths = parse_args(&args)?;
    let paths = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    let mut output = String::new();
    let mut errors = Vec::new();

    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        match render_target(path) {
            Ok(text) => output.push_str(&text),
            Err(err) => errors.push(err),
        }
    }

    print!("{output}");

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

    Ok(paths)
}

fn render_target(path: &str) -> Result<String, AppletError> {
    let path_ref = Path::new(path);
    let metadata = fs::symlink_metadata(path_ref)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?;

    let mut output = String::new();
    output.push_str(path);
    output.push('\n');

    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        render_directory(path_ref, "", &mut output)?;
    }

    Ok(output)
}

fn render_directory(path: &Path, prefix: &str, output: &mut String) -> Result<(), AppletError> {
    let mut entries = fs::read_dir(path)
        .map_err(|err| {
            AppletError::from_io(APPLET, "reading", Some(&path.display().to_string()), err)
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            AppletError::from_io(APPLET, "reading", Some(&path.display().to_string()), err)
        })?
        .into_iter()
        .map(|entry| {
            let child_path = entry.path();
            let metadata = fs::symlink_metadata(&child_path)?;
            Ok::<Entry, std::io::Error>(Entry {
                name: String::from_utf8_lossy(entry.file_name().as_os_str().as_bytes())
                    .into_owned(),
                path: child_path,
                metadata,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            AppletError::from_io(APPLET, "reading", Some(&path.display().to_string()), err)
        })?;

    entries.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));

    for (index, entry) in entries.iter().enumerate() {
        let is_last = index + 1 == entries.len();
        output.push_str(prefix);
        output.push_str(if is_last { "`-- " } else { "|-- " });
        output.push_str(&entry.name);

        if entry.metadata.file_type().is_symlink() {
            let target = fs::read_link(&entry.path)
                .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&entry.name), err))?;
            output.push_str(" -> ");
            output.push_str(&target.display().to_string());
        }
        output.push('\n');

        if entry.metadata.is_dir() && !entry.metadata.file_type().is_symlink() {
            let mut child_prefix = String::from(prefix);
            child_prefix.push_str(if is_last { "    " } else { "|   " });
            render_directory(&entry.path, &child_prefix, output)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::render_target;

    #[test]
    fn renders_file_target() {
        let dir = crate::common::unix::temp_dir("tree-file");
        let path = dir.join("file");
        std::fs::write(&path, b"x").unwrap();

        assert_eq!(
            render_target(path.to_str().unwrap()).unwrap(),
            format!("{}\n", path.display())
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn renders_directory_children_sorted() {
        let dir = crate::common::unix::temp_dir("tree-dir");
        std::fs::write(dir.join("b"), b"x").unwrap();
        std::fs::write(dir.join("a"), b"x").unwrap();

        let rendered = render_target(dir.to_str().unwrap()).unwrap();
        assert!(rendered.contains("|-- a\n`-- b\n") || rendered.contains("|-- a\n|-- b\n"));

        std::fs::remove_dir_all(dir).ok();
    }
}
