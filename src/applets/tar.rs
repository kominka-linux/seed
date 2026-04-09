use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};

use bzip2::Compression as Bzip2Compression;
use bzip2::bufread::MultiBzDecoder;
use bzip2::write::BzEncoder;
use flate2::Compression;
use flate2::bufread::MultiGzDecoder;
use flate2::write::GzEncoder;
use lzma_rust2::{XzOptions, XzReader, XzWriter};
use tar::{Archive, Builder, EntryType, Header};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, open_input, stdout};
use crate::common::unix::{self, FileKind};

const APPLET: &str = "tar";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Create,
    Extract,
    List,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompressionMode {
    Gzip,
    Bzip2,
    Xz,
}

#[derive(Debug, Default)]
struct Options {
    mode: Option<Mode>,
    archive: Option<String>,
    compression: Option<CompressionMode>,
    to_stdout: bool,
    verbose: bool,
    keep_old: bool,
    overwrite: bool,
    chdir: Option<PathBuf>,
    excludes: Vec<String>,
    operands: Vec<Operand>,
    members: Vec<String>,
}

#[derive(Debug)]
struct Operand {
    base_dir: Option<PathBuf>,
    name: String,
}

struct ArchiveName {
    path: PathBuf,
    stripped_prefix: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    match options.mode.expect("validated mode") {
        Mode::Create => create_archive(&options).map_err(|err| vec![err]),
        Mode::Extract => extract_archive(&options).map_err(|err| vec![err]),
        Mode::List => list_archive(&options).map_err(|err| vec![err]),
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parsing_flags = true;
    let mut pending_archive = false;
    let mut pending_chdir = false;
    let mut pending_exclude = false;
    let mut active_dir: Option<PathBuf> = None;

    for (index, arg) in args.iter().enumerate() {
        if parsing_flags && pending_archive {
            options.archive = Some(arg.clone());
            pending_archive = false;
            continue;
        }
        if parsing_flags && pending_chdir {
            let dir = PathBuf::from(arg);
            active_dir = Some(match active_dir.take() {
                Some(base) => base.join(dir),
                None => dir,
            });
            pending_chdir = false;
            options.chdir = active_dir.clone();
            continue;
        }
        if parsing_flags && pending_exclude {
            options.excludes.extend(read_exclude_file(arg)?);
            pending_exclude = false;
            continue;
        }

        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with("--") {
            match arg.as_str() {
                "--overwrite" => {
                    options.overwrite = true;
                    continue;
                }
                _ => return Err(vec![AppletError::unrecognized_option(APPLET, arg)]),
            }
        }

        let old_style = parsing_flags && index == 0 && !arg.starts_with('-');
        if parsing_flags && ((arg.starts_with('-') && arg.len() > 1) || old_style) {
            let flags = if old_style { arg.as_str() } else { &arg[1..] };
            for flag in flags.chars() {
                match flag {
                    'c' => set_mode(&mut options, Mode::Create)?,
                    'x' => set_mode(&mut options, Mode::Extract)?,
                    't' => set_mode(&mut options, Mode::List)?,
                    'f' => pending_archive = true,
                    'j' => set_compression(&mut options, CompressionMode::Bzip2)?,
                    'k' => options.keep_old = true,
                    'O' => options.to_stdout = true,
                    'v' => options.verbose = true,
                    'C' => pending_chdir = true,
                    'X' => pending_exclude = true,
                    'J' => set_compression(&mut options, CompressionMode::Xz)?,
                    'z' => set_compression(&mut options, CompressionMode::Gzip)?,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        match options.mode {
            Some(Mode::Create) => options.operands.push(Operand {
                base_dir: active_dir.clone(),
                name: arg.clone(),
            }),
            Some(Mode::Extract | Mode::List) => options.members.push(arg.clone()),
            None => return Err(vec![AppletError::new(APPLET, "need at least one option")]),
        }
    }

    if pending_archive || pending_chdir || pending_exclude {
        return Err(vec![AppletError::new(
            APPLET,
            "option requires an argument",
        )]);
    }

    if options.mode.is_none() {
        return Err(vec![AppletError::new(APPLET, "need at least one option")]);
    }

    Ok(options)
}

fn set_mode(options: &mut Options, mode: Mode) -> Result<(), Vec<AppletError>> {
    match options.mode {
        Some(existing) if existing != mode => Err(vec![AppletError::new(
            APPLET,
            "exactly one of 'c', 't' or 'x' must be specified",
        )]),
        _ => {
            options.mode = Some(mode);
            Ok(())
        }
    }
}

fn set_compression(
    options: &mut Options,
    compression: CompressionMode,
) -> Result<(), Vec<AppletError>> {
    match options.compression {
        Some(existing) if existing != compression => Err(vec![AppletError::new(
            APPLET,
            "only one compression option may be specified",
        )]),
        _ => {
            options.compression = Some(compression);
            Ok(())
        }
    }
}

fn create_archive(options: &Options) -> Result<(), AppletError> {
    if options.operands.is_empty() {
        return Err(AppletError::new(APPLET, "no files to archive"));
    }

    match options.archive.as_deref() {
        Some("-") | None => {
            let writer = stdout();
            match options.compression {
                Some(CompressionMode::Gzip) => {
                    let encoder = GzEncoder::new(writer, Compression::new(6));
                    write_archive(encoder, options)
                }
                Some(CompressionMode::Bzip2) => {
                    let encoder = BzEncoder::new(writer, Bzip2Compression::new(6));
                    write_archive(encoder, options)
                }
                Some(CompressionMode::Xz) => {
                    let encoder = XzWriter::new(writer, XzOptions::with_preset(6))
                        .map(|writer| writer.auto_finish())
                        .map_err(|err| AppletError::from_io(APPLET, "creating", None, err))?;
                    write_archive(encoder, options)
                }
                None => write_archive(writer, options),
            }
        }
        Some(path) => {
            let file = File::create(path)
                .map_err(|err| AppletError::from_io(APPLET, "creating", Some(path), err))?;
            let writer = BufWriter::with_capacity(BUFFER_SIZE, file);
            match options.compression {
                Some(CompressionMode::Gzip) => {
                    let encoder = GzEncoder::new(writer, Compression::new(6));
                    write_archive(encoder, options)
                }
                Some(CompressionMode::Bzip2) => {
                    let encoder = BzEncoder::new(writer, Bzip2Compression::new(6));
                    write_archive(encoder, options)
                }
                Some(CompressionMode::Xz) => {
                    let encoder = XzWriter::new(writer, XzOptions::with_preset(6))
                        .map(|writer| writer.auto_finish())
                        .map_err(|err| AppletError::from_io(APPLET, "creating", Some(path), err))?;
                    write_archive(encoder, options)
                }
                None => write_archive(writer, options),
            }
        }
    }
}

fn write_archive<W: Write>(writer: W, options: &Options) -> Result<(), AppletError> {
    let mut builder = Builder::new(writer);
    builder.follow_symlinks(false);
    let mut seen_hardlinks = HashMap::new();

    for operand in &options.operands {
        let source = source_path(operand);
        let archive_name = sanitize_archive_name(&operand.name)?;
        if let Some(prefix) = &archive_name.stripped_prefix {
            eprintln!("{APPLET}: removing leading '{prefix}' from member names");
        }
        append_operand(&mut builder, &source, &archive_name, &mut seen_hardlinks)
            .map_err(|err| AppletError::from_io(APPLET, "archiving", Some(&operand.name), err))?;
    }

    builder
        .into_inner()
        .map_err(|err| AppletError::from_io(APPLET, "writing", options.archive.as_deref(), err))?;
    Ok(())
}

fn append_operand<W: Write>(
    builder: &mut Builder<W>,
    source: &Path,
    archive_name: &ArchiveName,
    seen_hardlinks: &mut HashMap<(u64, u64), PathBuf>,
) -> io::Result<()> {
    append_path(builder, source, &archive_name.path, seen_hardlinks)
}

fn append_path<W: Write>(
    builder: &mut Builder<W>,
    source: &Path,
    archive_name: &Path,
    seen_hardlinks: &mut HashMap<(u64, u64), PathBuf>,
) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        if !archive_name.as_os_str().is_empty() {
            builder.append_dir(archive_name, source)?;
        }
        let mut entries = std::fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_name = entry.file_name();
            let child_source = entry.path();
            let child_archive = if archive_name.as_os_str().is_empty() {
                PathBuf::from(file_name)
            } else {
                archive_name.join(file_name)
            };
            append_path(builder, &child_source, &child_archive, seen_hardlinks)?;
        }
        Ok(())
    } else {
        match hardlink_target(&metadata, archive_name, seen_hardlinks) {
            Some(target) => append_hardlink(builder, &metadata, archive_name, &target),
            None => {
                builder.append_path_with_name(source, archive_name)?;
                remember_hardlink(&metadata, archive_name, seen_hardlinks);
                Ok(())
            }
        }
    }
}

fn hardlink_target(
    metadata: &std::fs::Metadata,
    archive_name: &Path,
    seen_hardlinks: &HashMap<(u64, u64), PathBuf>,
) -> Option<PathBuf> {
    if !should_track_hardlink(metadata) {
        return None;
    }
    let key = hardlink_key(metadata);
    seen_hardlinks
        .get(&key)
        .cloned()
        .filter(|target| target != archive_name)
        .or_else(|| seen_hardlinks.get(&key).cloned())
}

fn remember_hardlink(
    metadata: &std::fs::Metadata,
    archive_name: &Path,
    seen_hardlinks: &mut HashMap<(u64, u64), PathBuf>,
) {
    if should_track_hardlink(metadata) {
        seen_hardlinks
            .entry(hardlink_key(metadata))
            .or_insert_with(|| archive_name.to_path_buf());
    }
}

fn should_track_hardlink(metadata: &std::fs::Metadata) -> bool {
    metadata.nlink() > 1 && metadata.is_file()
}

fn hardlink_key(metadata: &std::fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn append_hardlink<W: Write>(
    builder: &mut Builder<W>,
    metadata: &std::fs::Metadata,
    archive_name: &Path,
    target: &Path,
) -> io::Result<()> {
    let mut header = Header::new_gnu();
    header.set_metadata(metadata);
    header.set_entry_type(EntryType::Link);
    header.set_size(0);
    builder.append_link(&mut header, archive_name, target)
}

fn sanitize_archive_name(name: &str) -> Result<ArchiveName, AppletError> {
    let mut parts = name.split('/').collect::<Vec<_>>();
    let stripped_prefix = parts.iter().rposition(|part| *part == "..").map(|index| {
        let prefix = format!("{}/", parts[..=index].join("/"));
        parts.drain(..=index);
        prefix
    });

    let normalized = parts
        .into_iter()
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        if name.split('/').all(|part| part.is_empty() || part == ".") {
            return Ok(ArchiveName {
                path: PathBuf::new(),
                stripped_prefix,
            });
        }
        return Err(AppletError::new(
            APPLET,
            format!("{name}: refusing to store empty member name"),
        ));
    }

    let mut path = PathBuf::new();
    for part in normalized {
        path.push(part);
    }

    Ok(ArchiveName {
        path,
        stripped_prefix,
    })
}

fn extract_archive(options: &Options) -> Result<(), AppletError> {
    let input = open_archive_input(options)?;
    let destination = options.chdir.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut output = if options.to_stdout {
        Some(stdout())
    } else {
        None
    };

    match input {
        ArchiveInput::Plain(reader) => {
            extract_from_reader(reader, &destination, options, output.as_mut())
        }
        ArchiveInput::Gzip(reader) => {
            extract_from_reader(reader, &destination, options, output.as_mut())
        }
        ArchiveInput::Bzip2(reader) => {
            extract_from_reader(reader, &destination, options, output.as_mut())
        }
        ArchiveInput::Xz(reader) => {
            extract_from_reader(reader, &destination, options, output.as_mut())
        }
    }
}

fn list_archive(options: &Options) -> Result<(), AppletError> {
    let input = open_archive_input(options)?;
    match input {
        ArchiveInput::Plain(reader) => list_from_reader(reader, options),
        ArchiveInput::Gzip(reader) => list_from_reader(reader, options),
        ArchiveInput::Bzip2(reader) => list_from_reader(reader, options),
        ArchiveInput::Xz(reader) => list_from_reader(reader, options),
    }
}

enum ArchiveInput {
    Plain(BufReader<Input>),
    Gzip(MultiGzDecoder<BufReader<Input>>),
    Bzip2(MultiBzDecoder<BufReader<Input>>),
    Xz(Box<XzReader<BufReader<Input>>>),
}

fn open_archive_input(options: &Options) -> Result<ArchiveInput, AppletError> {
    let archive = options.archive.as_deref().unwrap_or("-");
    let input = open_input(archive)
        .map_err(|err| AppletError::from_io(APPLET, "opening", Some(archive), err))?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, input);
    let compression = match options.compression {
        Some(explicit) => Some(explicit),
        None => detect_compression(&mut reader)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(archive), err))?,
    };

    Ok(match compression {
        Some(CompressionMode::Gzip) => ArchiveInput::Gzip(MultiGzDecoder::new(reader)),
        Some(CompressionMode::Bzip2) => ArchiveInput::Bzip2(MultiBzDecoder::new(reader)),
        Some(CompressionMode::Xz) => ArchiveInput::Xz(Box::new(XzReader::new(reader, true))),
        None => ArchiveInput::Plain(reader),
    })
}

fn detect_compression<R: BufRead>(reader: &mut R) -> io::Result<Option<CompressionMode>> {
    let header = reader.fill_buf()?;
    if header.starts_with(&[0x1f, 0x8b]) {
        Ok(Some(CompressionMode::Gzip))
    } else if header.starts_with(b"BZh") {
        Ok(Some(CompressionMode::Bzip2))
    } else if header.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        Ok(Some(CompressionMode::Xz))
    } else {
        Ok(None)
    }
}

fn extract_from_reader<R: Read>(
    reader: R,
    destination: &Path,
    options: &Options,
    mut stdout_writer: Option<&mut BufWriter<io::Stdout>>,
) -> Result<(), AppletError> {
    let reader = ensure_archive_has_data(reader, options.archive.as_deref())?;
    let mut archive = Archive::new(reader);
    let selectors = selectors(&options.members, &options.excludes);
    let mut matched = vec![false; selectors.len()];
    let mut deferred_dirs = Vec::new();

    let entries = archive
        .entries()
        .map_err(|err| read_error(options.archive.as_deref(), err))?;
    for entry_result in entries {
        let mut entry = entry_result.map_err(|err| read_error(options.archive.as_deref(), err))?;
        let path = entry
            .path()
            .map_err(|err| read_error(options.archive.as_deref(), err))?
            .into_owned();
        if is_apple_double(&path) {
            continue;
        }
        let path_text = display_entry_path(&path, entry.header().entry_type());
        if is_excluded(&path_text, &options.excludes) {
            continue;
        }
        if !selectors.is_empty() {
            let mut selected = false;
            for (index, selector) in selectors.iter().enumerate() {
                if matches_member(&path_text, selector) {
                    matched[index] = true;
                    selected = true;
                }
            }
            if !selected {
                continue;
            }
        }

        if options.verbose {
            println!("{path_text}");
        }

        if let Some(writer) = stdout_writer.as_deref_mut() {
            io::copy(&mut entry, writer)
                .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(&path_text), err))?;
        } else {
            extract_entry(
                &mut entry,
                destination,
                &path,
                &path_text,
                &mut deferred_dirs,
                options,
            )?;
        }
    }

    if let Some(writer) = stdout_writer {
        writer
            .flush()
            .map_err(|err| AppletError::from_io(APPLET, "flushing", None, err))?;
    } else {
        apply_deferred_directory_modes(&deferred_dirs)?;
    }

    ensure_members_found(&selectors, &matched)
}

struct DeferredDirectory {
    path: PathBuf,
    mode: u32,
}

fn extract_entry<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    deferred_dirs: &mut Vec<DeferredDirectory>,
    options: &Options,
) -> Result<(), AppletError> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_dir() {
        let output_path = archive_path_in(destination, path, path_text)?;
        if options.keep_old && output_path.exists() {
            return Ok(());
        }
        fs::create_dir_all(&output_path)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))?;
        let mode = entry
            .header()
            .mode()
            .map_err(|err| read_error(Some(path_text), err))?;
        deferred_dirs.push(DeferredDirectory {
            path: output_path,
            mode,
        });
        return Ok(());
    }

    if entry_type.is_symlink() {
        return extract_symlink(entry, destination, path, path_text, options);
    }

    if entry_type.is_hard_link() {
        return extract_hardlink(entry, destination, path, path_text, options);
    }

    if options.keep_old {
        let output_path = archive_path_in(destination, path, path_text)?;
        if path_exists(&output_path)? {
            return drain_entry(entry, path_text);
        }
    }

    if options.overwrite && entry_type.is_file() {
        return extract_regular_file_overwrite(entry, destination, path, path_text);
    }

    entry
        .unpack_in(destination)
        .map(|_| ())
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
}

fn extract_symlink<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    options: &Options,
) -> Result<(), AppletError> {
    let output_path = archive_path_in(destination, path, path_text)?;
    if options.keep_old && path_exists(&output_path)? {
        return drain_entry(entry, path_text);
    }
    ensure_parent_dir(&output_path, path_text)?;
    remove_existing_path(&output_path, path_text)?;
    let Some(target) = entry
        .link_name()
        .map_err(|err| read_error(Some(path_text), err))?
    else {
        return Err(AppletError::new(
            APPLET,
            format!("extracting {path_text}: missing symlink target"),
        ));
    };
    symlink(&target, &output_path)
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
}

fn extract_hardlink<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    options: &Options,
) -> Result<(), AppletError> {
    let output_path = archive_path_in(destination, path, path_text)?;
    if options.keep_old && path_exists(&output_path)? {
        return drain_entry(entry, path_text);
    }
    ensure_parent_dir(&output_path, path_text)?;
    let Some(target) = entry
        .link_name()
        .map_err(|err| read_error(Some(path_text), err))?
    else {
        return Err(AppletError::new(
            APPLET,
            format!("extracting {path_text}: missing hardlink target"),
        ));
    };
    let target_path = archive_path_in(destination, &target, path_text)?;
    if output_path == target_path {
        if output_path.exists() {
            return Ok(());
        }
        return Err(AppletError::new(
            APPLET,
            format!("extracting {path_text}: missing hardlink target"),
        ));
    }
    remove_existing_path(&output_path, path_text)?;
    let target_metadata = fs::symlink_metadata(&target_path)
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))?;
    if target_metadata.file_type().is_symlink() {
        let link_target = fs::read_link(&target_path)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))?;
        symlink(&link_target, &output_path)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
    } else {
        fs::hard_link(&target_path, &output_path)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
    }
}

fn extract_regular_file_overwrite<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
) -> Result<(), AppletError> {
    let output_path = archive_path_in(destination, path, path_text)?;
    ensure_parent_dir(&output_path, path_text)?;

    match fs::symlink_metadata(&output_path) {
        Ok(metadata) if metadata.is_dir() => {
            return Err(AppletError::new(
                APPLET,
                format!("extracting {path_text}: cannot overwrite directory"),
            ));
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            remove_existing_path(&output_path, path_text)?;
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(AppletError::from_io(
                APPLET,
                "extracting",
                Some(path_text),
                err,
            ));
        }
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&output_path)
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))?;
    io::copy(entry, &mut file)
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))?;

    let mode = entry
        .header()
        .mode()
        .map_err(|err| read_error(Some(path_text), err))?;
    fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
}

fn archive_path_in(
    destination: &Path,
    archive_path: &Path,
    display: &str,
) -> Result<PathBuf, AppletError> {
    let mut output = destination.to_path_buf();
    for component in archive_path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => output.push(part),
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(AppletError::new(
                    APPLET,
                    format!("extracting {display}: invalid path"),
                ));
            }
        }
    }
    Ok(output)
}

fn ensure_parent_dir(path: &Path, display: &str) -> Result<(), AppletError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(display), err))?;
    }
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, AppletError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(AppletError::from_io(
            APPLET,
            "extracting",
            Some(path.to_string_lossy().as_ref()),
            err,
        )),
    }
}

fn drain_entry<R: Read>(entry: &mut tar::Entry<'_, R>, path_text: &str) -> Result<(), AppletError> {
    io::copy(entry, &mut io::sink())
        .map(|_| ())
        .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(path_text), err))
}

fn remove_existing_path(path: &Path, display: &str) -> Result<(), AppletError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path)
                .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(display), err))
        }
        Ok(_) => fs::remove_file(path)
            .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(display), err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(AppletError::from_io(
            APPLET,
            "extracting",
            Some(display),
            err,
        )),
    }
}

fn apply_deferred_directory_modes(directories: &[DeferredDirectory]) -> Result<(), AppletError> {
    for directory in directories.iter().rev() {
        fs::set_permissions(&directory.path, fs::Permissions::from_mode(directory.mode)).map_err(
            |err| {
                AppletError::from_io(
                    APPLET,
                    "extracting",
                    Some(directory.path.to_string_lossy().as_ref()),
                    err,
                )
            },
        )?;
    }
    Ok(())
}

fn list_from_reader<R: Read>(reader: R, options: &Options) -> Result<(), AppletError> {
    let mut reader = ensure_archive_has_data(reader, options.archive.as_deref())?;
    let selectors = selectors(&options.members, &options.excludes);
    let mut matched = vec![false; selectors.len()];

    let mut long_path = None;
    let mut long_link = None;
    let mut pax_global = PaxHeaders::default();
    let mut pax_local = None;

    while let Some(entry) = read_list_entry(
        &mut reader,
        &mut long_path,
        &mut long_link,
        &mut pax_global,
        &mut pax_local,
        options,
    )? {
        let path = PathBuf::from(&entry.path);
        if is_apple_double(&path) {
            continue;
        }
        if is_excluded(&entry.path, &options.excludes) {
            continue;
        }
        if !selectors.is_empty() {
            let mut selected = false;
            for (index, selector) in selectors.iter().enumerate() {
                if matches_member(&entry.path, selector) {
                    matched[index] = true;
                    selected = true;
                }
            }
            if !selected {
                continue;
            }
        }
        if options.verbose {
            println!("{}", format_list_entry(&entry)?);
        } else {
            println!("{}", entry.path);
        }
    }

    ensure_members_found(&selectors, &matched)
}

fn ensure_archive_has_data<R: Read>(
    reader: R,
    archive: Option<&str>,
) -> Result<BufReader<R>, AppletError> {
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, reader);
    if reader
        .fill_buf()
        .map_err(|err| read_error(archive, err))?
        .is_empty()
    {
        return Err(short_read_error());
    }
    Ok(reader)
}

fn short_read_error() -> AppletError {
    AppletError::new(APPLET, "short read")
}

fn read_error(archive: Option<&str>, err: io::Error) -> AppletError {
    if err.kind() == io::ErrorKind::UnexpectedEof {
        short_read_error()
    } else {
        AppletError::from_io(APPLET, "reading", archive, err)
    }
}

struct ListEntry {
    path: String,
    link_name: Option<String>,
    entry_type: EntryType,
    mode: u32,
    size: u64,
    mtime: u64,
    username: String,
    groupname: String,
}

#[derive(Clone, Debug, Default)]
struct PaxHeaders {
    path: Option<String>,
    link_name: Option<String>,
    size: Option<u64>,
    mtime: Option<u64>,
    username: Option<String>,
    groupname: Option<String>,
}

fn read_list_entry<R: BufRead>(
    reader: &mut R,
    long_path: &mut Option<Vec<u8>>,
    long_link: &mut Option<Vec<u8>>,
    pax_global: &mut PaxHeaders,
    pax_local: &mut Option<PaxHeaders>,
    options: &Options,
) -> Result<Option<ListEntry>, AppletError> {
    loop {
        let block = match read_header_block(reader, options.archive.as_deref())? {
            Some(block) => block,
            None => return Ok(None),
        };

        let header = Header::from_byte_slice(&block);
        validate_header_checksum(header, options.archive.as_deref())?;
        let entry_type = header.entry_type();
        let header_size = header.entry_size().map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;

        if entry_type.is_gnu_longname() {
            *long_path = Some(read_entry_bytes(
                reader,
                header_size,
                true,
                options.archive.as_deref(),
            )?);
            continue;
        }
        if entry_type.is_gnu_longlink() {
            *long_link = Some(read_entry_bytes(
                reader,
                header_size,
                true,
                options.archive.as_deref(),
            )?);
            continue;
        }
        if entry_type.is_pax_global_extensions() || entry_type.is_pax_local_extensions() {
            let data = read_entry_bytes(reader, header_size, false, options.archive.as_deref())?;
            let headers = parse_pax_headers(&data, options.archive.as_deref())?;
            if entry_type.is_pax_global_extensions() {
                pax_global.apply(headers);
            } else {
                *pax_local = Some(headers);
            }
            continue;
        }

        let mut pax_headers = pax_global.clone();
        if let Some(local) = pax_local.take() {
            pax_headers.apply(local);
        }

        let mut path = path_from_header(header, long_path.take(), pax_headers.path.take())
            .map_err(|err| {
                AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
            })?;
        if entry_type.is_dir() && !path.ends_with('/') {
            path.push('/');
        }

        let link_name =
            link_name_from_header(header, long_link.take(), pax_headers.link_name.take()).map_err(
                |err| AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err),
            )?;
        let mode = header.mode().map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        let mtime = pax_headers.mtime.unwrap_or(header.mtime().map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?);
        let username = pax_headers.username.unwrap_or_else(|| {
            header
                .username()
                .ok()
                .flatten()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    header
                        .uid()
                        .map(|uid| uid.to_string())
                        .unwrap_or_else(|_| String::from("0"))
                })
        });
        let groupname = pax_headers.groupname.unwrap_or_else(|| {
            header
                .groupname()
                .ok()
                .flatten()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    header
                        .gid()
                        .map(|gid| gid.to_string())
                        .unwrap_or_else(|_| String::from("0"))
                })
        });
        let size = if entry_type.is_symlink() || entry_type.is_hard_link() {
            0
        } else {
            pax_headers.size.unwrap_or(header_size)
        };

        let skip_size = if entry_type.is_symlink() || entry_type.is_hard_link() {
            0
        } else {
            header_size
        };
        skip_entry_data(reader, skip_size, options.archive.as_deref())?;

        return Ok(Some(ListEntry {
            path,
            link_name,
            entry_type,
            mode,
            size,
            mtime,
            username,
            groupname,
        }));
    }
}

impl PaxHeaders {
    fn apply(&mut self, other: PaxHeaders) {
        if other.path.is_some() {
            self.path = other.path;
        }
        if other.link_name.is_some() {
            self.link_name = other.link_name;
        }
        if other.size.is_some() {
            self.size = other.size;
        }
        if other.mtime.is_some() {
            self.mtime = other.mtime;
        }
        if other.username.is_some() {
            self.username = other.username;
        }
        if other.groupname.is_some() {
            self.groupname = other.groupname;
        }
    }
}

fn read_header_block<R: BufRead>(
    reader: &mut R,
    archive: Option<&str>,
) -> Result<Option<[u8; 512]>, AppletError> {
    let mut block = [0_u8; 512];
    let mut read = 0;
    while read < block.len() {
        match reader.read(&mut block[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => {
                return Err(short_read_error());
            }
            Ok(bytes) => read += bytes,
            Err(err) => return Err(read_error(archive, err)),
        }
    }

    if block.iter().all(|byte| *byte == 0) {
        return Ok(None);
    }

    Ok(Some(block))
}

fn read_entry_bytes<R: BufRead>(
    reader: &mut R,
    size: u64,
    trim_nul: bool,
    archive: Option<&str>,
) -> Result<Vec<u8>, AppletError> {
    let mut data = vec![0; size as usize];
    reader
        .read_exact(&mut data)
        .map_err(|err| read_error(archive, err))?;
    skip_entry_padding(reader, size, archive)?;
    if trim_nul && let Some(end) = data.iter().position(|byte| *byte == 0) {
        data.truncate(end);
    }
    Ok(data)
}

fn skip_entry_data<R: BufRead>(
    reader: &mut R,
    size: u64,
    archive: Option<&str>,
) -> Result<(), AppletError> {
    let mut remaining = size;
    while remaining > 0 {
        let available = reader.fill_buf().map_err(|err| read_error(archive, err))?;
        if available.is_empty() {
            return Err(short_read_error());
        }
        let consume = available.len().min(remaining as usize);
        reader.consume(consume);
        remaining -= consume as u64;
    }
    skip_entry_padding(reader, size, archive)
}

fn skip_entry_padding<R: BufRead>(
    reader: &mut R,
    size: u64,
    archive: Option<&str>,
) -> Result<(), AppletError> {
    let padded = size.div_ceil(512) * 512;
    let mut remaining = padded.saturating_sub(size);
    while remaining > 0 {
        let available = reader.fill_buf().map_err(|err| read_error(archive, err))?;
        if available.is_empty() {
            return Err(short_read_error());
        }
        let consume = available.len().min(remaining as usize);
        reader.consume(consume);
        remaining -= consume as u64;
    }
    Ok(())
}

fn validate_header_checksum(header: &Header, archive: Option<&str>) -> Result<(), AppletError> {
    let bytes = header.as_bytes();
    let sum = bytes[..148]
        .iter()
        .chain(std::iter::repeat_n(&b' ', 8))
        .chain(bytes[156..].iter())
        .fold(0_u32, |acc, byte| acc + u32::from(*byte));
    let cksum = header.cksum().map_err(|err| read_error(archive, err))?;
    if sum == cksum {
        Ok(())
    } else {
        Err(AppletError::new(
            APPLET,
            format!(
                "reading {}: archive header checksum mismatch",
                archive.unwrap_or("-")
            ),
        ))
    }
}

fn path_from_header(
    header: &Header,
    override_bytes: Option<Vec<u8>>,
    pax_override: Option<String>,
) -> io::Result<String> {
    match pax_override {
        Some(path) => Ok(path),
        None => match override_bytes {
            Some(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            None => Ok(header.path()?.to_string_lossy().into_owned()),
        },
    }
}

fn link_name_from_header(
    header: &Header,
    override_bytes: Option<Vec<u8>>,
    pax_override: Option<String>,
) -> io::Result<Option<String>> {
    match pax_override {
        Some(path) => Ok(Some(path)),
        None => match override_bytes {
            Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            None => Ok(header
                .link_name()?
                .map(|path| path.to_string_lossy().into_owned())),
        },
    }
}

fn parse_pax_headers(data: &[u8], archive: Option<&str>) -> Result<PaxHeaders, AppletError> {
    let mut headers = PaxHeaders::default();
    for record in data
        .split(|byte| *byte == b'\n')
        .filter(|record| !record.is_empty())
    {
        let Some(space) = record.iter().position(|byte| *byte == b' ') else {
            return Err(AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            ));
        };
        let Ok(length_text) = std::str::from_utf8(&record[..space]) else {
            return Err(AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            ));
        };
        let Ok(reported_len) = length_text.parse::<usize>() else {
            return Err(AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            ));
        };
        if reported_len != record.len() + 1 {
            return Err(AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            ));
        }
        let payload = &record[space + 1..];
        let Some(equals) = payload.iter().position(|byte| *byte == b'=') else {
            return Err(AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            ));
        };
        let key = std::str::from_utf8(&payload[..equals]).map_err(|_| {
            AppletError::from_io(
                APPLET,
                "reading",
                archive,
                io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
            )
        })?;
        match key {
            "path" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                headers.path = Some(value.to_owned());
            }
            "linkpath" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                headers.link_name = Some(value.to_owned());
            }
            "size" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                headers.size = value.parse::<u64>().ok();
            }
            "mtime" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                let whole = value.split_once('.').map(|(head, _)| head).unwrap_or(value);
                headers.mtime = whole.parse::<u64>().ok();
            }
            "uname" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                headers.username = Some(value.to_owned());
            }
            "gname" => {
                let value = std::str::from_utf8(&payload[equals + 1..]).map_err(|_| {
                    AppletError::from_io(
                        APPLET,
                        "reading",
                        archive,
                        io::Error::new(io::ErrorKind::InvalidData, "malformed pax extension"),
                    )
                })?;
                headers.groupname = Some(value.to_owned());
            }
            _ => {}
        }
    }
    Ok(headers)
}

fn format_list_entry(entry: &ListEntry) -> Result<String, AppletError> {
    let mut line = format!(
        "{} {}/{} {:>9} {} {}",
        format_mode(entry.entry_type, entry.mode),
        entry.username,
        entry.groupname,
        entry.size,
        unix::format_full_timestamp(APPLET, entry.mtime)?,
        entry.path
    );
    if let Some(link_name) = &entry.link_name
        && (entry.entry_type.is_symlink() || entry.entry_type.is_hard_link())
    {
        line.push_str(" -> ");
        line.push_str(link_name);
    }
    Ok(line)
}

fn format_mode(entry_type: EntryType, mode: u32) -> String {
    unix::format_mode(file_kind(entry_type), mode, None)
}

fn file_kind(entry_type: EntryType) -> FileKind {
    if entry_type.is_dir() {
        FileKind::Directory
    } else if entry_type.is_symlink() {
        FileKind::Symlink
    } else if entry_type.is_character_special() {
        FileKind::CharDevice
    } else if entry_type.is_block_special() {
        FileKind::BlockDevice
    } else if entry_type.is_fifo() {
        FileKind::Fifo
    } else {
        FileKind::Regular
    }
}

fn display_entry_path(path: &Path, entry_type: EntryType) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if entry_type.is_dir() && !text.ends_with('/') {
        text.push('/');
    }
    text
}

fn selectors(members: &[String], excludes: &[String]) -> Vec<String> {
    members
        .iter()
        .filter(|member| !is_excluded(member, excludes))
        .cloned()
        .collect()
}

fn ensure_members_found(selectors: &[String], matched: &[bool]) -> Result<(), AppletError> {
    if let Some(index) = matched.iter().position(|found| !found) {
        return Err(AppletError::new(
            APPLET,
            format!("{}: not found in archive", selectors[index]),
        ));
    }
    Ok(())
}

fn matches_member(path: &str, selector: &str) -> bool {
    path == selector
        || path
            .strip_prefix(selector)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_excluded(path: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|exclude| matches_member(path, exclude))
}

fn is_apple_double(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("._"))
}

fn read_exclude_file(path: &str) -> Result<Vec<String>, Vec<AppletError>> {
    let content = std::fs::read(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
    Ok(content
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let trimmed = trim_ascii_whitespace(line);
            (!trimmed.is_empty()).then(|| String::from_utf8_lossy(trimmed).into_owned())
        })
        .collect())
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|index| index + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn source_path(operand: &Operand) -> PathBuf {
    match &operand.base_dir {
        Some(base) => base.join(&operand.name),
        None => PathBuf::from(&operand.name),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompressionMode, ListEntry, Mode, PaxHeaders, detect_compression, ensure_archive_has_data,
        parse_args, read_list_entry, sanitize_archive_name,
    };
    use std::io::{self, Cursor};
    use tar::{Builder, Header};

    fn parse(input: &[&str]) -> super::Options {
        let args = input
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        parse_args(&args).expect("parse args")
    }

    fn list_entries(bytes: Vec<u8>) -> Vec<ListEntry> {
        let options = parse(&["tf", "-"]);
        let mut reader = ensure_archive_has_data(Cursor::new(bytes), Some("-")).expect("archive");
        let mut entries = Vec::new();
        let mut long_path = None;
        let mut long_link = None;
        let mut pax_global = PaxHeaders::default();
        let mut pax_local = None;

        while let Some(entry) = read_list_entry(
            &mut reader,
            &mut long_path,
            &mut long_link,
            &mut pax_global,
            &mut pax_local,
            &options,
        )
        .expect("read entry")
        {
            entries.push(entry);
        }

        entries
    }

    #[test]
    fn old_style_options_are_supported() {
        let options = parse(&["czf", "archive.tar.gz", "foo"]);
        assert_eq!(options.mode, Some(Mode::Create));
        assert_eq!(options.compression, Some(CompressionMode::Gzip));
        assert_eq!(options.archive.as_deref(), Some("archive.tar.gz"));
        assert_eq!(options.operands.len(), 1);
    }

    #[test]
    fn extract_members_collect_after_flags() {
        let options = parse(&["-xf", "archive.tar", "-C", "out", "foo", "bar"]);
        assert_eq!(options.mode, Some(Mode::Extract));
        assert_eq!(
            options.members,
            vec![String::from("foo"), String::from("bar")]
        );
        assert_eq!(
            options.chdir.expect("chdir"),
            std::path::PathBuf::from("out")
        );
    }

    #[test]
    fn compression_flags_are_parsed() {
        let options = parse(&["cJf", "archive.tar.xz", "foo"]);
        assert_eq!(options.compression, Some(CompressionMode::Xz));

        let options = parse(&["-cjf", "archive.tar.bz2", "foo"]);
        assert_eq!(options.compression, Some(CompressionMode::Bzip2));
    }

    #[test]
    fn keep_old_and_overwrite_flags_are_parsed() {
        let options = parse(&["-xkf", "archive.tar"]);
        assert!(options.keep_old);

        let options = parse(&["-xf", "archive.tar", "--overwrite"]);
        assert!(options.overwrite);
    }

    #[test]
    fn compressed_input_is_detected_by_magic() {
        let mut gzip = Cursor::new(vec![0x1f, 0x8b, 0x08]);
        assert_eq!(
            detect_compression(&mut gzip).expect("gzip detection"),
            Some(CompressionMode::Gzip)
        );

        let mut bzip2 = Cursor::new(b"BZh9".to_vec());
        assert_eq!(
            detect_compression(&mut bzip2).expect("bzip2 detection"),
            Some(CompressionMode::Bzip2)
        );

        let mut xz = Cursor::new(vec![0xfd, b'7', b'z', b'X', b'Z', 0x00]);
        assert_eq!(
            detect_compression(&mut xz).expect("xz detection"),
            Some(CompressionMode::Xz)
        );
    }

    #[test]
    fn empty_archive_is_rejected() {
        let err = ensure_archive_has_data(io::empty(), Some("-")).expect_err("empty archive");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn zero_filled_archive_is_accepted() {
        ensure_archive_has_data(Cursor::new(vec![0_u8; 1024]), Some("-"))
            .expect("zero-filled archive");
    }

    #[test]
    fn strips_leading_prefix_through_last_parent_dir() {
        let archive_name = sanitize_archive_name("./../tar.tempdir/input_dir/../input_dir")
            .expect("sanitize archive name");
        assert_eq!(archive_name.path, std::path::PathBuf::from("input_dir"));
        assert_eq!(
            archive_name.stripped_prefix.as_deref(),
            Some("./../tar.tempdir/input_dir/../")
        );
    }

    #[test]
    fn preserves_normal_relative_archive_name() {
        let archive_name = sanitize_archive_name("foo/./bar").expect("sanitize archive name");
        assert_eq!(archive_name.path, std::path::PathBuf::from("foo/bar"));
        assert_eq!(archive_name.stripped_prefix, None);
    }

    #[test]
    fn dot_archive_name_maps_to_archive_root() {
        let archive_name = sanitize_archive_name("./.").expect("sanitize archive name");
        assert!(archive_name.path.as_os_str().is_empty());
        assert_eq!(archive_name.stripped_prefix, None);
    }

    #[test]
    fn list_reader_applies_pax_path_extension() {
        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);
        let long_name = format!("dir/{}", "a".repeat(120));
        builder
            .append_pax_extensions([("path", long_name.as_bytes())])
            .expect("append pax");

        let mut header = Header::new_ustar();
        header.set_path("dir/short").expect("set path");
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &b"x"[..]).expect("append file");
        builder.finish().expect("finish archive");
        drop(builder);

        let entries = list_entries(bytes);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, long_name);
    }

    #[test]
    fn list_reader_applies_pax_linkpath_extension() {
        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);
        let long_target = "b".repeat(120);
        builder
            .append_pax_extensions([("linkpath", long_target.as_bytes())])
            .expect("append pax");

        let mut header = Header::new_ustar();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_path("dir/link").expect("set path");
        header.set_link_name("short").expect("set link name");
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append(&header, io::empty())
            .expect("append symlink");
        builder.finish().expect("finish archive");
        drop(builder);

        let entries = list_entries(bytes);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "dir/link");
        assert_eq!(entries[0].link_name.as_deref(), Some(long_target.as_str()));
    }
}
