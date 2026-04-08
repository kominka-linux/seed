use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

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

    for operand in &options.operands {
        let source = source_path(operand);
        let archive_name = Path::new(&operand.name);
        append_operand(&mut builder, &source, archive_name)
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
    archive_name: &Path,
) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        builder.append_dir_all(archive_name, source)
    } else {
        builder.append_path_with_name(source, archive_name)
    }
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
    let mut archive = Archive::new(reader);
    let selectors = selectors(&options.members, &options.excludes);
    let mut matched = vec![false; selectors.len()];

    let entries = archive
        .entries()
        .map_err(|err| AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err))?;
    for entry_result in entries {
        let mut entry = entry_result.map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        let path = entry
            .path()
            .map_err(|err| {
                AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
            })?
            .into_owned();
        if is_apple_double(&path) {
            continue;
        }
        let path_text = path.to_string_lossy().into_owned();
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
            entry
                .unpack_in(destination)
                .map_err(|err| AppletError::from_io(APPLET, "extracting", Some(&path_text), err))?;
        }
    }

    if let Some(writer) = stdout_writer {
        writer
            .flush()
            .map_err(|err| AppletError::from_io(APPLET, "flushing", None, err))?;
    }

    ensure_members_found(&selectors, &matched)
}

fn list_from_reader<R: Read>(reader: R, options: &Options) -> Result<(), AppletError> {
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, reader);
    let selectors = selectors(&options.members, &options.excludes);
    let mut matched = vec![false; selectors.len()];

    let mut long_path = None;
    let mut long_link = None;

    while let Some(entry) = read_list_entry(&mut reader, &mut long_path, &mut long_link, options)? {
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

fn read_list_entry<R: BufRead>(
    reader: &mut R,
    long_path: &mut Option<Vec<u8>>,
    long_link: &mut Option<Vec<u8>>,
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
                options.archive.as_deref(),
            )?);
            continue;
        }
        if entry_type.is_gnu_longlink() {
            *long_link = Some(read_entry_bytes(
                reader,
                header_size,
                options.archive.as_deref(),
            )?);
            continue;
        }

        let mut path = path_from_header(header, long_path.take()).map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        if entry_type.is_dir() && !path.ends_with('/') {
            path.push('/');
        }

        let link_name = link_name_from_header(header, long_link.take()).map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        let mode = header.mode().map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        let mtime = header.mtime().map_err(|err| {
            AppletError::from_io(APPLET, "reading", options.archive.as_deref(), err)
        })?;
        let username = header
            .username()
            .ok()
            .flatten()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                header
                    .uid()
                    .map(|uid| uid.to_string())
                    .unwrap_or_else(|_| String::from("0"))
            });
        let groupname = header
            .groupname()
            .ok()
            .flatten()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                header
                    .gid()
                    .map(|gid| gid.to_string())
                    .unwrap_or_else(|_| String::from("0"))
            });
        let size = if entry_type.is_symlink() || entry_type.is_hard_link() {
            0
        } else {
            header_size
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
                return Err(AppletError::new(
                    APPLET,
                    format!("reading {}: short read", archive.unwrap_or("-")),
                ));
            }
            Ok(bytes) => read += bytes,
            Err(err) => return Err(AppletError::from_io(APPLET, "reading", archive, err)),
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
    archive: Option<&str>,
) -> Result<Vec<u8>, AppletError> {
    let mut data = vec![0; size as usize];
    reader
        .read_exact(&mut data)
        .map_err(|err| AppletError::from_io(APPLET, "reading", archive, err))?;
    skip_entry_padding(reader, size, archive)?;
    if let Some(end) = data.iter().position(|byte| *byte == 0) {
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
        let available = reader
            .fill_buf()
            .map_err(|err| AppletError::from_io(APPLET, "reading", archive, err))?;
        if available.is_empty() {
            return Err(AppletError::new(
                APPLET,
                format!("reading {}: short read", archive.unwrap_or("-")),
            ));
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
        let available = reader
            .fill_buf()
            .map_err(|err| AppletError::from_io(APPLET, "reading", archive, err))?;
        if available.is_empty() {
            return Err(AppletError::new(
                APPLET,
                format!("reading {}: short read", archive.unwrap_or("-")),
            ));
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
    let cksum = header
        .cksum()
        .map_err(|err| AppletError::from_io(APPLET, "reading", archive, err))?;
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

fn path_from_header(header: &Header, override_bytes: Option<Vec<u8>>) -> io::Result<String> {
    match override_bytes {
        Some(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        None => Ok(header.path()?.to_string_lossy().into_owned()),
    }
}

fn link_name_from_header(
    header: &Header,
    override_bytes: Option<Vec<u8>>,
) -> io::Result<Option<String>> {
    match override_bytes {
        Some(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        None => Ok(header
            .link_name()?
            .map(|path| path.to_string_lossy().into_owned())),
    }
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
    } else if entry_type.is_hard_link() {
        FileKind::HardLink
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
    use super::{CompressionMode, Mode, detect_compression, parse_args};
    use std::io::Cursor;

    fn parse(input: &[&str]) -> super::Options {
        let args = input
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        parse_args(&args).expect("parse args")
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
}
