use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use zip::read::{ZipArchive, ZipFile};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::{copy_stream, stdout};

const APPLET: &str = "unzip";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Extract,
    List,
    Stdout,
}

#[derive(Debug, Eq, PartialEq)]
struct Options {
    archive: String,
    directory: Option<PathBuf>,
    members: Vec<String>,
    mode: Mode,
    overwrite: bool,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    match options.mode {
        Mode::Extract => extract_archive(&options),
        Mode::List => list_archive(&options),
        Mode::Stdout => write_archive_to_stdout(&options),
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut directory = None;
    let mut members = Vec::new();
    let mut mode = Mode::Extract;
    let mut overwrite = false;
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if cursor.parsing_flags() && arg == "-d" {
            directory = Some(PathBuf::from(cursor.next_value(APPLET, "d")?));
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'l' => mode = select_mode(mode, Mode::List)?,
                    'o' => overwrite = true,
                    'p' => mode = select_mode(mode, Mode::Stdout)?,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        members.push(arg.to_owned());
    }

    if members.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing archive operand")]);
    }

    let archive = members.remove(0);
    if archive == "-" {
        return Err(vec![AppletError::new(
            APPLET,
            "reading -: standard input is not supported",
        )]);
    }

    if directory.is_some() && mode != Mode::Extract {
        return Err(vec![AppletError::new(
            APPLET,
            "-d is only valid when extracting files",
        )]);
    }

    Ok(Options {
        archive,
        directory,
        members,
        mode,
        overwrite,
    })
}

fn select_mode(current: Mode, next: Mode) -> Result<Mode, Vec<AppletError>> {
    if current == Mode::Extract || current == next {
        Ok(next)
    } else {
        Err(vec![AppletError::new(
            APPLET,
            "only one of -l or -p may be specified",
        )])
    }
}

fn list_archive(options: &Options) -> AppletResult {
    let mut archive = open_archive(&options.archive)?;
    let mut out = stdout();
    let (seen, errors) = visit_matching_entries(&mut archive, options, |entry| {
        writeln!(out, "{}\t{}", entry.size(), entry.name())
            .map_err(|err| AppletError::from_io(APPLET, "writing stdout", None, err))
    })?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "flushing stdout", None, err)])?;
    finish_entry_results(seen, errors, options)
}

fn write_archive_to_stdout(options: &Options) -> AppletResult {
    let mut archive = open_archive(&options.archive)?;
    let mut out = stdout();
    let (seen, errors) = visit_matching_entries(&mut archive, options, |entry| {
        copy_stream(entry, &mut out)
            .map(|_| ())
            .map_err(|err| AppletError::from_io(APPLET, "writing stdout", Some(entry.name()), err))
    })?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "flushing stdout", None, err)])?;
    finish_entry_results(seen, errors, options)
}

fn extract_archive(options: &Options) -> AppletResult {
    let mut archive = open_archive(&options.archive)?;
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    let base = options
        .directory
        .as_deref()
        .unwrap_or_else(|| Path::new("."));

    if let Some(dir) = &options.directory
        && let Err(err) = fs::create_dir_all(dir)
    {
        return Err(vec![AppletError::from_io(
            APPLET,
            "creating",
            Some(path_for_error(dir)),
            err,
        )]);
    }

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| vec![archive_error(&options.archive, err)])?;
        if !matches_member(&options.members, entry.name()) {
            continue;
        }
        seen.insert(entry.name().to_owned());
        if let Err(err) = extract_entry(&mut entry, base, options.overwrite) {
            errors.push(err);
        }
    }

    finish_entry_results(seen, errors, options)
}

fn visit_matching_entries(
    archive: &mut ZipArchive<File>,
    options: &Options,
    mut visit: impl FnMut(&mut ZipFile<'_>) -> Result<(), AppletError>,
) -> Result<(BTreeSet<String>, Vec<AppletError>), Vec<AppletError>> {
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| vec![archive_error(&options.archive, err)])?;
        if !matches_member(&options.members, entry.name()) {
            continue;
        }
        seen.insert(entry.name().to_owned());
        if let Err(err) = visit(&mut entry) {
            errors.push(err);
        }
    }

    Ok((seen, errors))
}

fn finish_entry_results(
    seen: BTreeSet<String>,
    mut errors: Vec<AppletError>,
    options: &Options,
) -> AppletResult {
    for member in missing_members(&options.members, &seen) {
        errors.push(AppletError::new(
            APPLET,
            format!("{member}: not found in {}", options.archive),
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn missing_members<'a>(members: &'a [String], seen: &BTreeSet<String>) -> Vec<&'a str> {
    members
        .iter()
        .map(String::as_str)
        .filter(|member| !seen.contains(*member))
        .collect()
}

fn matches_member(members: &[String], name: &str) -> bool {
    members.is_empty() || members.iter().any(|member| member == name)
}

fn open_archive(path: &str) -> Result<ZipArchive<File>, Vec<AppletError>> {
    let file = File::open(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(path), err)])?;
    ZipArchive::new(file).map_err(|err| vec![archive_error(path, err)])
}

fn archive_error(path: &str, err: zip::result::ZipError) -> AppletError {
    AppletError::new(APPLET, format!("reading {path}: {err}"))
}

fn extract_entry(entry: &mut ZipFile<'_>, base: &Path, overwrite: bool) -> Result<(), AppletError> {
    let relative = entry.enclosed_name().ok_or_else(|| {
        AppletError::new(APPLET, format!("unsafe path in archive: {}", entry.name()))
    })?;
    let output = base.join(relative);

    if entry.is_dir() {
        ensure_relative_directories(base, relative)?;
        set_permissions(&output, entry);
        return Ok(());
    }

    if let Some(parent) = relative.parent() {
        ensure_relative_directories(base, parent)?;
    }

    let metadata = match fs::symlink_metadata(&output) {
        Ok(metadata) => Some(metadata),
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => {
            return Err(AppletError::from_io(
                APPLET,
                "checking",
                Some(path_for_error(&output)),
                err,
            ));
        }
    };

    if let Some(metadata) = metadata {
        if metadata.file_type().is_symlink() {
            return Err(AppletError::new(
                APPLET,
                format!("refusing to overwrite symlink {}", path_for_error(&output)),
            ));
        }
        if metadata.is_dir() {
            return Err(AppletError::new(
                APPLET,
                format!("creating {}: Is a directory", path_for_error(&output)),
            ));
        }
        if !overwrite {
            return Err(AppletError::new(
                APPLET,
                format!("creating {}: File exists", path_for_error(&output)),
            ));
        }
    }

    let file = if overwrite {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&output)
    } else {
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&output)
    }
    .map_err(|err| AppletError::from_io(APPLET, "creating", Some(path_for_error(&output)), err))?;
    let mut writer = BufWriter::new(file);
    copy_stream(entry, &mut writer).map_err(|err| {
        AppletError::from_io(APPLET, "extracting", Some(path_for_error(&output)), err)
    })?;
    writer.flush().map_err(|err| {
        AppletError::from_io(APPLET, "flushing", Some(path_for_error(&output)), err)
    })?;
    set_permissions(&output, entry);
    Ok(())
}

fn ensure_relative_directories(base: &Path, relative: &Path) -> Result<(), AppletError> {
    let mut current = base.to_path_buf();

    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(part) => current.push(part),
            _ => {
                return Err(AppletError::new(
                    APPLET,
                    format!("unsafe path in archive: {}", relative.display()),
                ));
            }
        }

        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppletError::new(
                    APPLET,
                    format!(
                        "refusing to write through symlink {}",
                        path_for_error(&current)
                    ),
                ));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(AppletError::new(
                    APPLET,
                    format!("creating {}: Not a directory", path_for_error(&current)),
                ));
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|create_err| {
                    AppletError::from_io(
                        APPLET,
                        "creating",
                        Some(path_for_error(&current)),
                        create_err,
                    )
                })?;
            }
            Err(err) => {
                return Err(AppletError::from_io(
                    APPLET,
                    "checking",
                    Some(path_for_error(&current)),
                    err,
                ));
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn set_permissions(path: &Path, entry: &ZipFile<'_>) {
    if let Some(mode) = entry.unix_mode() {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _entry: &ZipFile<'_>) {}

fn path_for_error(path: &Path) -> &str {
    path.to_str().unwrap_or("<non-utf8 path>")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Write};

    use zip::CompressionMethod;
    use zip::write::FileOptions;

    use super::{Mode, Options, extract_archive, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn archive_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, contents) in entries {
            writer.start_file(*name, options).expect("start file");
            writer.write_all(contents).expect("write contents");
        }
        writer.finish().expect("finish zip").into_inner()
    }

    #[test]
    fn parse_args_handles_extract_mode_and_members() {
        let options = parse_args(&args(&["-o", "-d", "out", "archive.zip", "hello.txt"]))
            .expect("parse unzip");

        assert_eq!(
            options,
            Options {
                archive: "archive.zip".to_owned(),
                directory: Some("out".into()),
                members: vec!["hello.txt".to_owned()],
                mode: Mode::Extract,
                overwrite: true,
            }
        );
    }

    #[test]
    fn parse_args_rejects_conflicting_modes() {
        let err = parse_args(&args(&["-lp", "archive.zip"])).expect_err("reject modes");
        assert_eq!(
            err[0].to_string(),
            "unzip: only one of -l or -p may be specified"
        );
    }

    #[test]
    fn extract_archive_rejects_unsafe_paths() {
        let dir = crate::common::unix::temp_dir("unzip");
        let archive = dir.join("archive.zip");
        fs::write(&archive, archive_bytes(&[("../escape.txt", b"blocked\n")])).expect("write zip");

        let err = extract_archive(&Options {
            archive: archive.display().to_string(),
            directory: Some(dir.join("out")),
            members: Vec::new(),
            mode: Mode::Extract,
            overwrite: false,
        })
        .expect_err("reject unsafe path");

        assert_eq!(
            err[0].to_string(),
            "unzip: unsafe path in archive: ../escape.txt"
        );
        assert!(!dir.join("escape.txt").exists());

        let _ = fs::remove_dir_all(dir);
    }
}
