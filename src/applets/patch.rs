use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use diffy::{Patch, apply_bytes};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::fs::remove_path;
use crate::common::io::stdin;

const APPLET: &str = "patch";
const DEV_NULL: &[u8] = b"/dev/null";

#[derive(Debug, Default, Eq, PartialEq)]
struct Options {
    input: Option<String>,
    strip: usize,
    reverse: bool,
    ignore_already_applied: bool,
    target: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    let patch_bytes = read_patch_input(options.input.as_deref())?;
    let chunks = split_patch_chunks(&patch_bytes);

    if chunks.is_empty() {
        return Err(vec![AppletError::new(APPLET, "no patch data found")]);
    }

    for chunk in &chunks {
        apply_chunk(chunk, &options)?;
    }

    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut operands = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if !cursor.parsing_flags() || !arg.starts_with('-') || arg == "-" {
            operands.push(arg.to_owned());
            continue;
        }

        if arg == "-i" {
            options.input = Some(cursor.next_value(APPLET, "i")?.to_owned());
            continue;
        }
        if arg == "-p" {
            let value = cursor.next_value(APPLET, "p")?;
            options.strip = parse_strip(value)?;
            continue;
        }
        if let Some(value) = arg.strip_prefix("-p")
            && !value.is_empty()
        {
            options.strip = parse_strip(value)?;
            continue;
        }

        for flag in arg[1..].chars() {
            match flag {
                'N' => options.ignore_already_applied = true,
                'R' => options.reverse = true,
                _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            }
        }
    }

    match (options.input.is_some(), operands.len()) {
        (false, 0) | (true, 0) => {}
        (false, 1) | (true, 1) => options.target = Some(operands.remove(0)),
        (false, 2) => {
            options.target = Some(operands.remove(0));
            options.input = Some(operands.remove(0));
        }
        _ => return Err(vec![AppletError::new(APPLET, "too many operands")]),
    }

    Ok(options)
}

fn parse_strip(value: &str) -> Result<usize, Vec<AppletError>> {
    value.parse::<usize>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid strip count '{value}'"),
        )]
    })
}

fn read_patch_input(path: Option<&str>) -> Result<Vec<u8>, Vec<AppletError>> {
    let mut bytes = Vec::new();
    if let Some(path) = path {
        fs::File::open(path)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
    } else {
        stdin()
            .read_to_end(&mut bytes)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading stdin", None, err)])?;
    }
    Ok(bytes)
}

fn apply_chunk(chunk: &[u8], options: &Options) -> AppletResult {
    let raw_patch = Patch::from_bytes(chunk)
        .map_err(|err| vec![AppletError::new(APPLET, format!("parsing patch: {err}"))])?;
    let target = target_path(&raw_patch, options)?;
    let patch = if options.reverse {
        raw_patch.reverse()
    } else {
        raw_patch
    };
    let display = target.display().to_string();
    let creates_file = patch_creates_file(&patch);
    let deletes_file = patch_deletes_file(&patch);
    let file_exists = target.exists();

    if creates_file && !file_exists {
        eprintln!("creating {display}");
    } else {
        eprintln!("patching file {display}");
    }

    let base = if file_exists {
        fs::read(&target)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&display), err)])?
    } else if creates_file {
        Vec::new()
    } else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("can't open '{display}': No such file or directory"),
        )]);
    };

    let result = apply_bytes(&base, &patch);
    let already_applied = result.is_err() && apply_bytes(&base, &patch.reverse()).is_ok();

    let patched = match result {
        Ok(bytes) => bytes,
        Err(_) if already_applied && options.ignore_already_applied => return Ok(()),
        Err(_) if already_applied => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("reversed or previously applied patch detected for {display}"),
            )]);
        }
        Err(err) => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("failed to apply patch to {display}: {err}"),
            )]);
        }
    };

    if deletes_file && patched.is_empty() {
        remove_path(&target, false, false).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "removing",
                Some(&display),
                err,
            )]
        })?;
        return Ok(());
    }

    write_target(&target, &patched).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "writing",
            Some(target.to_string_lossy().as_ref()),
            err,
        )]
    })
}

fn write_target(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(contents)?;
    file.flush()
}

fn target_path(patch: &Patch<'_, [u8]>, options: &Options) -> Result<PathBuf, Vec<AppletError>> {
    if let Some(target) = &options.target {
        return Ok(PathBuf::from(target));
    }

    let candidate = patch
        .modified()
        .filter(|path| *path != DEV_NULL)
        .or_else(|| patch.original().filter(|path| *path != DEV_NULL))
        .ok_or_else(|| vec![AppletError::new(APPLET, "patch does not name a file")])?;

    let stripped = strip_components(candidate, options.strip)?;
    Ok(PathBuf::from(OsStr::from_bytes(&stripped)))
}

fn strip_components(path: &[u8], count: usize) -> Result<Vec<u8>, Vec<AppletError>> {
    if path == DEV_NULL {
        return Ok(path.to_vec());
    }

    let mut remainder = path;
    for _ in 0..count {
        while matches!(remainder.first(), Some(b'/')) {
            remainder = &remainder[1..];
        }
        let Some(index) = remainder.iter().position(|byte| *byte == b'/') else {
            return Err(vec![AppletError::new(
                APPLET,
                format!(
                    "can't strip {count} leading component(s) from '{}'",
                    String::from_utf8_lossy(path)
                ),
            )]);
        };
        remainder = &remainder[index + 1..];
    }

    while matches!(remainder.first(), Some(b'/')) {
        remainder = &remainder[1..];
    }

    if remainder.is_empty() {
        return Err(vec![AppletError::new(
            APPLET,
            "patch path is empty after stripping",
        )]);
    }

    Ok(remainder.to_vec())
}

fn patch_creates_file(patch: &Patch<'_, [u8]>) -> bool {
    patch.original() == Some(DEV_NULL)
}

fn patch_deletes_file(patch: &Patch<'_, [u8]>) -> bool {
    patch.modified() == Some(DEV_NULL)
}

fn split_patch_chunks(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut chunks = Vec::new();
    let mut current_start = None;
    let mut in_hunk = false;
    let mut offset = 0usize;

    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let line_start = offset;
        offset += line.len();

        let starts_file = line.starts_with(b"--- ");
        let starts_hunk = line.starts_with(b"@@ ");
        let is_hunk_body = matches!(line.first(), Some(b' ' | b'+' | b'-' | b'\\'));

        match (current_start, in_hunk) {
            (None, _) if starts_file => {
                current_start = Some(line_start);
                in_hunk = false;
            }
            (Some(start), false) if starts_hunk => {
                in_hunk = true;
                current_start = Some(start);
            }
            (Some(start), true) if starts_file => {
                chunks.push(bytes[start..line_start].to_vec());
                current_start = Some(line_start);
                in_hunk = false;
            }
            (Some(start), true) if !starts_hunk && !is_hunk_body => {
                chunks.push(bytes[start..line_start].to_vec());
                current_start = None;
                in_hunk = false;
            }
            _ => {}
        }
    }

    if let Some(start) = current_start {
        chunks.push(bytes[start..].to_vec());
    }

    if chunks.is_empty() && !bytes.is_empty() {
        chunks.push(bytes.to_vec());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Options, apply_chunk, parse_args, split_patch_chunks, strip_components};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parse_args_supports_strip_and_input() {
        let options =
            parse_args(&args(&["-p1", "-R", "-N", "target", "update.patch"])).expect("parse patch");
        assert_eq!(
            options,
            Options {
                input: Some("update.patch".to_owned()),
                strip: 1,
                reverse: true,
                ignore_already_applied: true,
                target: Some("target".to_owned()),
            }
        );
    }

    #[test]
    fn strip_components_preserves_remaining_separators() {
        let stripped = strip_components(b"bogus_dir///dir2///file", 1).expect("strip path");
        assert_eq!(stripped, b"dir2///file");
    }

    #[test]
    fn split_patch_chunks_handles_multiple_files() {
        let bytes = b"--- a/one\n+++ b/one\n@@ -1 +1 @@\n-a\n+b\n--- a/two\n+++ b/two\n@@ -1 +1 @@\n-c\n+d\n";
        let chunks = split_patch_chunks(bytes);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn apply_chunk_creates_new_file() {
        let dir = crate::common::unix::temp_dir("patch");
        let patch = b"--- /dev/null\n+++ testfile\n@@ -0,0 +1 @@\n+qwerty\n";

        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("chdir");

        let result = apply_chunk(
            patch,
            &Options {
                input: None,
                strip: 0,
                reverse: false,
                ignore_already_applied: false,
                target: None,
            },
        );

        std::env::set_current_dir(previous).expect("restore cwd");
        assert!(result.is_ok());
        assert_eq!(
            fs::read(dir.join("testfile")).expect("read file"),
            b"qwerty\n"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
