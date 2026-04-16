use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
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

use crate::common::args::ArgCursor;
use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::fs::AtomicFile;
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
    strip_components: usize,
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

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    match options.mode.expect("validated mode") {
        Mode::Create => create_archive(&options).map_err(|err| vec![err]),
        Mode::Extract => extract_archive(&options).map_err(|err| vec![err]),
        Mode::List => list_archive(&options).map_err(|err| vec![err]),
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut pending_archive = false;
    let mut pending_chdir = false;
    let mut pending_exclude = false;
    let mut pending_strip_components = false;
    let mut active_dir: Option<PathBuf> = None;
    let start = if let Some(first) = args.first() {
        let first = crate::common::args::os_to_string(APPLET, first.as_os_str())?;
        if !first.starts_with('-') {
            let flags = first;
            for flag in flags.chars() {
                apply_short_flag(
                    &mut options,
                    flag,
                    &mut pending_archive,
                    &mut pending_chdir,
                    &mut pending_exclude,
                )?;
            }
            1
        } else {
            0
        }
    } else {
        0
    };

    let mut cursor = ArgCursor::new(&args[start..]);
    while let Some(arg) = cursor.next_token(APPLET)? {
        if pending_archive {
            options.archive = Some(arg.to_string());
            pending_archive = false;
            continue;
        }
        if pending_chdir {
            let dir = PathBuf::from(arg);
            active_dir = Some(match active_dir.take() {
                Some(base) => base.join(dir),
                None => dir,
            });
            pending_chdir = false;
            options.chdir = active_dir.clone();
            continue;
        }
        if pending_exclude {
            options.excludes.extend(read_exclude_file(arg)?);
            pending_exclude = false;
            continue;
        }
        if pending_strip_components {
            options.strip_components = parse_strip_components(arg)?;
            pending_strip_components = false;
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with("--") {
            if let Some(value) = arg.strip_prefix("--strip-components=") {
                options.strip_components = parse_strip_components(value)?;
                continue;
            }
            match arg {
                "--overwrite" => {
                    options.overwrite = true;
                    continue;
                }
                "--strip-components" => {
                    pending_strip_components = true;
                    continue;
                }
                _ => return Err(vec![AppletError::unrecognized_option(APPLET, arg)]),
            }
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                apply_short_flag(
                    &mut options,
                    flag,
                    &mut pending_archive,
                    &mut pending_chdir,
                    &mut pending_exclude,
                )?;
            }
            continue;
        }

        match options.mode {
            Some(Mode::Create) => options.operands.push(Operand {
                base_dir: active_dir.clone(),
                name: arg.to_string(),
            }),
            Some(Mode::Extract | Mode::List) => options.members.push(arg.to_string()),
            None => return Err(vec![AppletError::new(APPLET, "need at least one option")]),
        }
    }

    if pending_archive || pending_chdir || pending_exclude || pending_strip_components {
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

fn apply_short_flag(
    options: &mut Options,
    flag: char,
    pending_archive: &mut bool,
    pending_chdir: &mut bool,
    pending_exclude: &mut bool,
) -> Result<(), Vec<AppletError>> {
    match flag {
        'c' => set_mode(options, Mode::Create),
        'x' => set_mode(options, Mode::Extract),
        't' => set_mode(options, Mode::List),
        'f' => {
            *pending_archive = true;
            Ok(())
        }
        'j' => set_compression(options, CompressionMode::Bzip2),
        'k' => {
            options.keep_old = true;
            Ok(())
        }
        'O' => {
            options.to_stdout = true;
            Ok(())
        }
        'v' => {
            options.verbose = true;
            Ok(())
        }
        'C' => {
            *pending_chdir = true;
            Ok(())
        }
        'X' => {
            *pending_exclude = true;
            Ok(())
        }
        'J' => set_compression(options, CompressionMode::Xz),
        'z' => set_compression(options, CompressionMode::Gzip),
        _ => Err(vec![AppletError::invalid_option(APPLET, flag)]),
    }
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

fn parse_strip_components(value: &str) -> Result<usize, Vec<AppletError>> {
    value.parse::<usize>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid --strip-components value '{value}'"),
        )]
    })
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

    let writer = open_archive_output(options)?;
    write_archive(writer, options)
}

fn open_archive_output(options: &Options) -> Result<Box<dyn Write>, AppletError> {
    let destination = options.archive.as_deref().filter(|path| *path != "-");
    let writer: Box<dyn Write> = match destination {
        Some(path) => {
            let file = File::create(path)
                .map_err(|err| AppletError::from_io(APPLET, "creating", Some(path), err))?;
            Box::new(BufWriter::with_capacity(BUFFER_SIZE, file))
        }
        None => Box::new(stdout()),
    };
    wrap_archive_output(writer, options.compression, destination)
}

fn wrap_archive_output(
    writer: Box<dyn Write>,
    compression: Option<CompressionMode>,
    destination: Option<&str>,
) -> Result<Box<dyn Write>, AppletError> {
    match compression {
        Some(CompressionMode::Gzip) => Ok(Box::new(GzEncoder::new(writer, Compression::new(6)))),
        Some(CompressionMode::Bzip2) => {
            Ok(Box::new(BzEncoder::new(writer, Bzip2Compression::new(6))))
        }
        Some(CompressionMode::Xz) => XzWriter::new(writer, XzOptions::with_preset(6))
            .map(|writer| Box::new(writer.auto_finish()) as Box<dyn Write>)
            .map_err(|err| AppletError::from_io(APPLET, "creating", destination, err)),
        None => Ok(writer),
    }
}

fn write_archive(writer: Box<dyn Write>, options: &Options) -> Result<(), AppletError> {
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
    metadata.nlink() > 1 && (metadata.is_file() || metadata.file_type().is_symlink())
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

    extract_from_reader(input, &destination, options, output.as_mut())
}

fn list_archive(options: &Options) -> Result<(), AppletError> {
    let input = open_archive_input(options)?;
    list_from_reader(input, options)
}

enum ArchiveInput {
    Plain(BufReader<Input>),
    Gzip(MultiGzDecoder<BufReader<Input>>),
    Bzip2(MultiBzDecoder<BufReader<Input>>),
    Xz(Box<XzReader<BufReader<Input>>>),
}

impl Read for ArchiveInput {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(reader) => reader.read(buf),
            Self::Gzip(reader) => reader.read(buf),
            Self::Bzip2(reader) => reader.read(buf),
            Self::Xz(reader) => reader.read(buf),
        }
    }
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

    Ok(wrap_archive_input(reader, compression))
}

fn wrap_archive_input(reader: BufReader<Input>, compression: Option<CompressionMode>) -> ArchiveInput {
    match compression {
        Some(CompressionMode::Gzip) => ArchiveInput::Gzip(MultiGzDecoder::new(reader)),
        Some(CompressionMode::Bzip2) => ArchiveInput::Bzip2(MultiBzDecoder::new(reader)),
        Some(CompressionMode::Xz) => ArchiveInput::Xz(Box::new(XzReader::new(reader, true))),
        None => ArchiveInput::Plain(reader),
    }
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

        let Some(stripped_path) = strip_archive_path(&path, options.strip_components) else {
            continue;
        };

        if options.verbose {
            println!("{path_text}");
        }

        if let Some(writer) = stdout_writer.as_deref_mut() {
            io::copy(&mut entry, writer).map_err(|err| extract_io_error(&path_text, err))?;
        } else {
            extract_entry(
                &mut entry,
                destination,
                &stripped_path,
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
        let output_path = prepare_archive_path(destination, path, path_text, true)?;
        if options.keep_old && output_path.exists() {
            return Ok(());
        }
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

    if entry_type.is_file() {
        return extract_regular_file(entry, destination, path, path_text, options);
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
        .map_err(|err| extract_io_error(path_text, err))
}

fn extract_regular_file<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    options: &Options,
) -> Result<(), AppletError> {
    let output_path = archive_path_in(destination, path, path_text)?;
    if output_path == destination {
        return drain_entry(entry, path_text);
    }

    if options.keep_old && path_exists(&output_path)? {
        return drain_entry(entry, path_text);
    }

    if options.overwrite {
        return extract_regular_file_overwrite(entry, destination, path, path_text);
    }

    prepare_archive_parents(destination, path, path_text)?;

    match fs::symlink_metadata(&output_path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
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
        Err(err) => return Err(extract_io_error(path_text, err)),
    }

    let mut output = AtomicFile::new(&output_path).map_err(|err| extract_io_error(path_text, err))?;
    io::copy(entry, output.file_mut()).map_err(|err| extract_io_error(path_text, err))?;
    output
        .commit()
        .map_err(|err| extract_io_error(path_text, err))?;

    let mode = entry
        .header()
        .mode()
        .map_err(|err| read_error(Some(path_text), err))?;
    fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
        .map_err(|err| extract_io_error(path_text, err))
}

fn extract_symlink<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    options: &Options,
) -> Result<(), AppletError> {
    let output_path = prepare_archive_path(destination, path, path_text, false)?;
    if options.keep_old && path_exists(&output_path)? {
        return drain_entry(entry, path_text);
    }
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
    symlink(&target, &output_path).map_err(|err| extract_io_error(path_text, err))
}

fn extract_hardlink<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
    options: &Options,
) -> Result<(), AppletError> {
    let output_path = prepare_archive_path(destination, path, path_text, false)?;
    if options.keep_old && path_exists(&output_path)? {
        return drain_entry(entry, path_text);
    }
    let Some(target) = entry
        .link_name()
        .map_err(|err| read_error(Some(path_text), err))?
    else {
        return Err(AppletError::new(
            APPLET,
            format!("extracting {path_text}: missing hardlink target"),
        ));
    };
    let Some(target) = strip_archive_path(&target, options.strip_components) else {
        return Err(AppletError::new(
            APPLET,
            format!("extracting {path_text}: missing hardlink target"),
        ));
    };
    let target_path = archive_path_in(destination, &target, path_text)?;
    validate_archive_ancestors(destination, &target, path_text, false)?;
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
    fs::hard_link(&target_path, &output_path).map_err(|err| extract_io_error(path_text, err))
}

fn extract_regular_file_overwrite<R: Read>(
    entry: &mut tar::Entry<'_, R>,
    destination: &Path,
    path: &Path,
    path_text: &str,
) -> Result<(), AppletError> {
    let output_path = prepare_archive_path(destination, path, path_text, false)?;

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
        Ok(_) => {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&output_path)
                .map_err(|err| extract_io_error(path_text, err))?;
            io::copy(entry, &mut output).map_err(|err| extract_io_error(path_text, err))?;

            let mode = entry
                .header()
                .mode()
                .map_err(|err| read_error(Some(path_text), err))?;
            return fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
                .map_err(|err| extract_io_error(path_text, err));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(extract_io_error(path_text, err));
        }
    }

    let mut output = AtomicFile::new(&output_path).map_err(|err| extract_io_error(path_text, err))?;
    io::copy(entry, output.file_mut()).map_err(|err| extract_io_error(path_text, err))?;
    output
        .commit()
        .map_err(|err| extract_io_error(path_text, err))?;

    let mode = entry
        .header()
        .mode()
        .map_err(|err| read_error(Some(path_text), err))?;
    fs::set_permissions(&output_path, fs::Permissions::from_mode(mode))
        .map_err(|err| extract_io_error(path_text, err))
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

fn prepare_archive_path(
    destination: &Path,
    archive_path: &Path,
    display: &str,
    create_leaf_dir: bool,
) -> Result<PathBuf, AppletError> {
    let output_path = archive_path_in(destination, archive_path, display)?;
    validate_archive_ancestors(destination, archive_path, display, create_leaf_dir)?;
    Ok(output_path)
}

fn prepare_archive_parents(
    destination: &Path,
    archive_path: &Path,
    display: &str,
) -> Result<(), AppletError> {
    validate_archive_ancestors(destination, archive_path, display, false)
}

fn validate_archive_ancestors(
    destination: &Path,
    archive_path: &Path,
    display: &str,
    create_leaf_dir: bool,
) -> Result<(), AppletError> {
    let mut current = destination.to_path_buf();
    let mut components = archive_path.components().peekable();

    while let Some(component) = components.next() {
        match component {
            Component::CurDir => continue,
            Component::Normal(part) => {
                current.push(part);
                let is_last = components.peek().is_none();
                if is_last && !create_leaf_dir {
                    continue;
                }

                match fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(AppletError::new(
                            APPLET,
                            format!(
                                "extracting {display}: trying to unpack outside of destination path"
                            ),
                        ));
                    }
                    Ok(metadata) if metadata.is_dir() => {}
                    Ok(_) => {
                        return Err(AppletError::new(
                            APPLET,
                            format!("extracting {display}: Not a directory"),
                        ));
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        fs::create_dir(&current).map_err(|create_err| {
                            AppletError::from_io(
                                APPLET,
                                "extracting",
                                Some(display),
                                create_err,
                            )
                        })?;
                    }
                    Err(err) => return Err(extract_io_error(display, err)),
                }
            }
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(AppletError::new(
                    APPLET,
                    format!("extracting {display}: invalid path"),
                ));
            }
        }
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
        .map_err(|err| extract_io_error(path_text, err))
}

fn remove_existing_path(path: &Path, display: &str) -> Result<(), AppletError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|err| extract_io_error(display, err))
        }
        Ok(_) => fs::remove_file(path).map_err(|err| extract_io_error(display, err)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(extract_io_error(display, err)),
    }
}

fn extract_io_error(path: &str, err: io::Error) -> AppletError {
    if is_short_read_io_error(&err) {
        short_read_error()
    } else {
        AppletError::from_io(APPLET, "extracting", Some(path), err)
    }
}

fn is_short_read_io_error(err: &io::Error) -> bool {
    let mut current: Option<&(dyn Error + 'static)> = Some(err);
    while let Some(source) = current {
        let message = source.to_string();
        if message.contains("failed to read entire block") || message.contains("unexpected EOF") {
            return true;
        }
        if let Some(io_error) = source.downcast_ref::<io::Error>() {
            if io_error.kind() == io::ErrorKind::UnexpectedEof {
                return true;
            }
            if let Some(inner) = io_error.get_ref() {
                current = Some(inner);
                continue;
            }
        }
        current = source.source();
    }

    false
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
    if is_short_read_io_error(&err) {
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

fn strip_archive_path(path: &Path, strip_components: usize) -> Option<PathBuf> {
    if strip_components == 0 {
        return Some(path.to_path_buf());
    }

    let mut skipped = 0usize;
    let mut stripped = PathBuf::new();
    for part in path.to_string_lossy().split('/') {
        if part.is_empty() {
            continue;
        }
        if skipped < strip_components {
            skipped += 1;
            continue;
        }
        stripped.push(part);
    }

    (skipped == strip_components && !stripped.as_os_str().is_empty()).then_some(stripped)
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
        list_from_reader, parse_args, read_list_entry, sanitize_archive_name,
    };
    use bzip2::bufread::MultiBzDecoder;
    use flate2::bufread::MultiGzDecoder;
    use lzma_rust2::XzReader;
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt;
    use std::fs;
    use std::io::{self, BufReader, Cursor};
    use tar::{Builder, Header};

    fn parse(input: &[&str]) -> super::Options {
        let args = input
            .iter()
            .map(OsString::from)
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

    fn list_result<R: io::Read>(reader: R) -> Result<(), super::AppletError> {
        let options = parse(&["tf", "-"]);
        list_from_reader(reader, &options)
    }

    fn extract_result<R: io::Read>(reader: R) -> Result<(), super::AppletError> {
        let options = parse(&["xf", "-"]);
        let destination = crate::common::unix::temp_dir("tar-extract-test");
        let result = super::extract_from_reader(reader, &destination, &options, None);
        let _ = fs::remove_dir_all(destination);
        result
    }

    fn extract_into(bytes: Vec<u8>, destination: &std::path::Path, overwrite: bool) -> Result<(), super::AppletError> {
        let options = if overwrite {
            parse(&["x", "--overwrite", "-f", "-"])
        } else {
            parse(&["xf", "-"])
        };
        super::extract_from_reader(Cursor::new(bytes), destination, &options, None)
    }

    fn raw_header(entry_type: tar::EntryType, size: u64) -> Header {
        let mut header = Header::new_ustar();
        header.set_entry_type(entry_type);
        header.set_size(size);
        header.set_mode(0o644);
        header.set_cksum();
        header
    }

    fn named_header(path: &str, entry_type: tar::EntryType, size: u64) -> Header {
        let mut header = raw_header(entry_type, size);
        header.set_path(path).expect("set path");
        header.set_cksum();
        header
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
    fn strip_components_is_parsed() {
        let options = parse(&["-xf", "archive.tar", "--strip-components=2"]);
        assert_eq!(options.strip_components, 2);

        let options = parse(&["-xf", "archive.tar", "--strip-components", "3"]);
        assert_eq!(options.strip_components, 3);
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
    fn strip_archive_path_counts_dot_component() {
        assert_eq!(
            super::strip_archive_path(std::path::Path::new("./foo/bar"), 1),
            Some(std::path::PathBuf::from("foo/bar"))
        );
    }

    #[test]
    fn strip_archive_path_skips_entries_with_too_few_components() {
        assert_eq!(super::strip_archive_path(std::path::Path::new("foo"), 1), None);
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

    #[test]
    fn truncated_gnu_longname_payload_is_short_read() {
        let mut bytes = Vec::new();
        let header = raw_header(tar::EntryType::GNULongName, 10);
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(b"short");

        let err = list_result(Cursor::new(bytes)).expect_err("expected short read");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn truncated_pax_payload_is_short_read() {
        let mut bytes = Vec::new();
        let header = raw_header(tar::EntryType::XHeader, 10);
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(b"12 path=x");

        let err = list_result(Cursor::new(bytes)).expect_err("expected short read");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn malformed_pax_payload_is_reported() {
        let mut bytes = Vec::new();
        let data = b"12 bad\n";
        let header = raw_header(tar::EntryType::XHeader, data.len() as u64);
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(512) * 512, 0);

        let err = list_result(Cursor::new(bytes)).expect_err("expected malformed pax error");
        assert_eq!(err.to_string(), "tar: reading -: malformed pax extension");
    }

    #[test]
    fn corrupt_gzip_tar_is_rejected() {
        let reader = MultiGzDecoder::new(BufReader::new(Cursor::new(b"not gzip".to_vec())));
        let err = list_result(reader).expect_err("expected gzip failure");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn corrupt_bzip2_tar_is_rejected() {
        let reader = MultiBzDecoder::new(BufReader::new(Cursor::new(b"not bzip2".to_vec())));
        let err = list_result(reader).expect_err("expected bzip2 failure");
        assert_eq!(err.to_string(), "tar: reading -: bzip2: bz2 header missing");
    }

    #[test]
    fn extract_applies_strip_components() {
        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);

        let mut header = Header::new_ustar();
        header.set_path("pkg/bin/tool").expect("set path");
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, &b"ok\n"[..]).expect("append file");
        builder.finish().expect("finish archive");
        drop(builder);

        let destination = crate::common::unix::temp_dir("tar-strip-components");
        let options = parse(&["xf", "-", "--strip-components=1"]);
        super::extract_from_reader(Cursor::new(bytes), &destination, &options, None)
            .expect("extract with strip components");

        assert_eq!(
            fs::read(destination.join("bin/tool")).expect("read extracted file"),
            b"ok\n"
        );
        assert!(!destination.join("pkg").exists());

        let _ = fs::remove_dir_all(destination);
    }

    #[test]
    fn corrupt_xz_tar_is_rejected() {
        let reader = XzReader::new(BufReader::new(Cursor::new(b"not xz".to_vec())), true);
        let err = list_result(reader).expect_err("expected xz failure");
        assert_eq!(err.to_string(), "tar: reading -: invalid XZ magic bytes");
    }

    #[test]
    fn truncated_file_body_is_short_read_on_extract() {
        let mut bytes = Vec::new();
        let mut header = Header::new_ustar();
        header.set_path("file").expect("set path");
        header.set_size(10);
        header.set_mode(0o644);
        header.set_cksum();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(b"hello");

        let err = extract_result(Cursor::new(bytes)).expect_err("expected short read");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn symlinked_parent_is_rejected_before_creating_subdirs() {
        let dir = crate::common::unix::temp_dir("tar-symlink-parent");
        let src_dir = dir.join("src");
        let outside = dir.join("outside");
        let destination = dir.join("dest");
        fs::create_dir_all(src_dir.join("dir/sub")).expect("create source tree");
        fs::create_dir_all(&outside).expect("create outside dir");
        fs::create_dir_all(&destination).expect("create destination dir");
        fs::write(src_dir.join("dir/sub/file"), b"data").expect("write file");
        std::os::unix::fs::symlink(&outside, destination.join("dir")).expect("create symlink");

        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);
        builder
            .append_path_with_name(src_dir.join("dir/sub/file"), "dir/sub/file")
            .expect("append archive entry");
        builder.finish().expect("finish archive");
        drop(builder);

        let err = extract_into(bytes, &destination, false).expect_err("expected escape rejection");
        assert_eq!(
            err.to_string(),
            "tar: extracting dir/sub/file: trying to unpack outside of destination path"
        );
        assert!(!outside.join("sub").exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn overwrite_mode_still_rejects_symlinked_parent() {
        let dir = crate::common::unix::temp_dir("tar-overwrite-symlink-parent");
        let src_dir = dir.join("src");
        let outside = dir.join("outside");
        let destination = dir.join("dest");
        fs::create_dir_all(src_dir.join("dir/sub")).expect("create source tree");
        fs::create_dir_all(outside.join("sub")).expect("create outside dir");
        fs::create_dir_all(&destination).expect("create destination dir");
        fs::write(src_dir.join("dir/sub/file"), b"data").expect("write file");
        fs::write(outside.join("sub/file"), b"keep").expect("write outside file");
        std::os::unix::fs::symlink(&outside, destination.join("dir")).expect("create symlink");

        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);
        builder
            .append_path_with_name(src_dir.join("dir/sub/file"), "dir/sub/file")
            .expect("append archive entry");
        builder.finish().expect("finish archive");
        drop(builder);

        let err = extract_into(bytes, &destination, true).expect_err("expected escape rejection");
        assert_eq!(
            err.to_string(),
            "tar: extracting dir/sub/file: trying to unpack outside of destination path"
        );
        assert_eq!(fs::read(outside.join("sub/file")).expect("read outside file"), b"keep");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn overwrite_mode_preserves_existing_hardlinks() {
        let dir = crate::common::unix::temp_dir("tar-overwrite-hardlinks");
        let source = dir.join("src");
        let destination = dir.join("dest");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(&destination).expect("create destination dir");

        let source_file = source.join("input_hard");
        fs::write(&source_file, b"Ok\n").expect("write archive source");

        let mut bytes = Vec::new();
        let mut builder = Builder::new(&mut bytes);
        builder
            .append_path_with_name(&source_file, "input_hard")
            .expect("append archive entry");
        builder.finish().expect("finish archive");
        drop(builder);

        let input = destination.join("input");
        let input_hard = destination.join("input_hard");
        fs::write(&input, b"Ok\n").expect("write destination file");
        fs::hard_link(&input, &input_hard).expect("create destination hardlink");
        fs::write(&input, b"WRONG\n").expect("mutate existing hardlink pair");

        extract_into(bytes, &destination, true).expect("extract archive");

        assert_eq!(fs::read(&input).expect("read input"), b"Ok\n");
        assert_eq!(fs::read(&input_hard).expect("read hardlink"), b"Ok\n");
        let input_meta = fs::metadata(&input).expect("input metadata");
        let hard_meta = fs::metadata(&input_hard).expect("hardlink metadata");
        assert_eq!(input_meta.ino(), hard_meta.ino());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_gzip_tar_is_rejected_on_extract() {
        let reader = MultiGzDecoder::new(BufReader::new(Cursor::new(b"not gzip".to_vec())));
        let err = extract_result(reader).expect_err("expected gzip failure");
        assert_eq!(err.to_string(), "tar: short read");
    }

    #[test]
    fn corrupt_bzip2_tar_is_rejected_on_extract() {
        let reader = MultiBzDecoder::new(BufReader::new(Cursor::new(b"not bzip2".to_vec())));
        let err = extract_result(reader).expect_err("expected bzip2 failure");
        assert_eq!(err.to_string(), "tar: reading -: bzip2: bz2 header missing");
    }

    #[test]
    fn corrupt_xz_tar_is_rejected_on_extract() {
        let reader = XzReader::new(BufReader::new(Cursor::new(b"not xz".to_vec())), true);
        let err = extract_result(reader).expect_err("expected xz failure");
        assert_eq!(err.to_string(), "tar: reading -: invalid XZ magic bytes");
    }

    #[test]
    fn invalid_hardlink_target_is_rejected_on_extract() {
        let mut bytes = Vec::new();
        let mut header = named_header("link", tar::EntryType::Link, 0);
        header
            .set_link_name("/tmp/outside")
            .expect("set link target");
        header.set_cksum();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(&[0_u8; 1024]);

        let err = extract_result(Cursor::new(bytes)).expect_err("expected invalid path");
        assert_eq!(err.to_string(), "tar: extracting link: invalid path");
    }

    #[test]
    fn header_checksum_mismatch_is_rejected_on_extract() {
        let mut bytes = vec![0_u8; 512];
        bytes[..4].copy_from_slice(b"file");
        bytes[100..108].copy_from_slice(b"0000644\0");
        bytes[124..136].copy_from_slice(b"00000000000\0");
        bytes[136..148].copy_from_slice(b"00000000000\0");
        bytes[156] = b'0';
        bytes[257..263].copy_from_slice(b"ustar\0");
        bytes[263..265].copy_from_slice(b"00");
        bytes[148..156].copy_from_slice(b"000000\0 ");
        bytes.extend_from_slice(&[0_u8; 1024]);

        let err = extract_result(Cursor::new(bytes)).expect_err("expected checksum mismatch");
        assert_eq!(
            err.to_string(),
            "tar: reading -: archive header checksum mismatch"
        );
    }
}
