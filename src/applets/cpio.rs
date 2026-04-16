use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::applet::finish;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "cpio";
const MODE_MASK: u32 = 0o170000;
#[cfg(target_os = "linux")]
const S_IFREG: u32 = libc::S_IFREG;
#[cfg(target_os = "macos")]
const S_IFREG: u32 = libc::S_IFREG as u32;
#[cfg(target_os = "linux")]
const S_IFDIR: u32 = libc::S_IFDIR;
#[cfg(target_os = "macos")]
const S_IFDIR: u32 = libc::S_IFDIR as u32;
#[cfg(target_os = "linux")]
const S_IFLNK: u32 = libc::S_IFLNK;
#[cfg(target_os = "macos")]
const S_IFLNK: u32 = libc::S_IFLNK as u32;
#[cfg(target_os = "linux")]
const S_IFIFO: u32 = libc::S_IFIFO;
#[cfg(target_os = "macos")]
const S_IFIFO: u32 = libc::S_IFIFO as u32;
#[cfg(target_os = "linux")]
const S_IFCHR: u32 = libc::S_IFCHR;
#[cfg(target_os = "macos")]
const S_IFCHR: u32 = libc::S_IFCHR as u32;
#[cfg(target_os = "linux")]
const S_IFBLK: u32 = libc::S_IFBLK;
#[cfg(target_os = "macos")]
const S_IFBLK: u32 = libc::S_IFBLK as u32;
const TRAILER: &str = "TRAILER!!!";
const NEWC_MAGIC: &str = "070701";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Mode {
    Extract,
    Create,
    List,
    PassThrough(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Owner {
    uid: u32,
    gid: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    mode: Option<Mode>,
    file: Option<String>,
    owner: Option<Owner>,
    members: Vec<String>,
    make_dirs: bool,
    preserve_mtime: bool,
    verbose: bool,
    overwrite: bool,
    dereference: bool,
    nul_terminated: bool,
    format_newc: bool,
}

#[derive(Clone, Debug)]
struct ArchiveEntry {
    ino: u32,
    mode: u32,
    uid: u32,
    gid: u32,
    nlink: u32,
    mtime: u32,
    dev_major: u32,
    dev_minor: u32,
    rdev_major: u32,
    rdev_minor: u32,
    name: String,
    data: Vec<u8>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let options = parse_args(&args)?;
    match options.mode.clone().ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            "need at least one of -i, -o, -t or -p",
        )]
    })? {
        Mode::Create => create_archive(&options),
        Mode::Extract => extract_archive(&options, Path::new(".")),
        Mode::List => list_archive(&options),
        Mode::PassThrough(destination) => pass_through(&options, &destination),
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        index += 1;

        if arg == "--" {
            options.members.extend(args[index..].iter().cloned());
            break;
        }

        if !arg.starts_with('-') || arg == "-" {
            options.members.push(arg.clone());
            continue;
        }

        let mut offset = 1;
        while offset < arg.len() {
            let flag = arg.as_bytes()[offset] as char;
            offset += 1;
            match flag {
                'i' => set_mode(&mut options, Mode::Extract)?,
                'o' => set_mode(&mut options, Mode::Create)?,
                't' => set_mode(&mut options, Mode::List)?,
                'd' => options.make_dirs = true,
                'm' => options.preserve_mtime = true,
                'v' => options.verbose = true,
                'u' => options.overwrite = true,
                'L' => options.dereference = true,
                '0' => options.nul_terminated = true,
                'H' => {
                    let value = attached_or_next(arg, &mut offset, args, &mut index, "H")?;
                    if value != "newc" && value != "crc" {
                        return Err(vec![AppletError::new(
                            APPLET,
                            format!("unsupported archive format '{value}'"),
                        )]);
                    }
                    options.format_newc = true;
                    break;
                }
                'F' => {
                    let value = attached_or_next(arg, &mut offset, args, &mut index, "F")?;
                    options.file = Some(value);
                    break;
                }
                'R' => {
                    let value = attached_or_next(arg, &mut offset, args, &mut index, "R")?;
                    options.owner = Some(parse_owner(&value)?);
                    break;
                }
                'p' => {
                    let value = if should_take_passthrough_arg_from_next(arg, offset) {
                        next_arg(args, &mut index, "p")?
                    } else {
                        let value = arg[offset..].to_string();
                        offset = arg.len();
                        value
                    };
                    set_mode(&mut options, Mode::PassThrough(PathBuf::from(value)))?;
                    if offset >= arg.len() {
                        break;
                    }
                }
                _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            }
        }
    }

    if matches!(options.mode, Some(Mode::Create)) && !options.format_newc {
        return Err(vec![AppletError::new(
            APPLET,
            "archive format must be specified as -H newc",
        )]);
    }

    Ok(options)
}

fn set_mode(options: &mut Options, mode: Mode) -> Result<(), Vec<AppletError>> {
    match (&options.mode, &mode) {
        (Some(existing), _) if !same_mode(existing, &mode) => Err(vec![AppletError::new(
            APPLET,
            "exactly one of -i, -o, -t or -p may be specified",
        )]),
        _ => {
            options.mode = Some(mode);
            Ok(())
        }
    }
}

fn same_mode(lhs: &Mode, rhs: &Mode) -> bool {
    matches!(
        (lhs, rhs),
        (Mode::Extract, Mode::Extract)
            | (Mode::Create, Mode::Create)
            | (Mode::List, Mode::List)
            | (Mode::PassThrough(_), Mode::PassThrough(_))
    )
}

fn attached_or_next(
    arg: &str,
    offset: &mut usize,
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, Vec<AppletError>> {
    if *offset < arg.len() {
        let value = arg[*offset..].to_string();
        *offset = arg.len();
        return Ok(value);
    }
    let Some(value) = args.get(*index) else {
        return Err(vec![AppletError::option_requires_arg(APPLET, option)]);
    };
    *index += 1;
    Ok(value.clone())
}

fn next_arg(args: &[String], index: &mut usize, option: &str) -> Result<String, Vec<AppletError>> {
    let Some(value) = args.get(*index) else {
        return Err(vec![AppletError::option_requires_arg(APPLET, option)]);
    };
    *index += 1;
    Ok(value.clone())
}

fn should_take_passthrough_arg_from_next(arg: &str, offset: usize) -> bool {
    let remainder = &arg[offset..];
    remainder.is_empty() || remainder.chars().all(is_short_flag_without_arg)
}

fn is_short_flag_without_arg(flag: char) -> bool {
    matches!(flag, 'i' | 'o' | 't' | 'd' | 'm' | 'v' | 'u' | 'L' | '0')
}

fn parse_owner(value: &str) -> Result<Owner, Vec<AppletError>> {
    let (user, group) = value
        .split_once(':')
        .map_or((value, None), |(u, g)| (u, Some(g)));
    let (uid, default_gid) = lookup_user(user)?;
    let gid = match group {
        Some(group) => lookup_group(group)?,
        None => default_gid,
    };
    Ok(Owner { uid, gid })
}

fn lookup_user(value: &str) -> Result<(u32, u32), Vec<AppletError>> {
    if let Ok(uid) = value.parse::<u32>() {
        return Ok((uid, uid));
    }
    let name = CString::new(value)
        .map_err(|_| vec![AppletError::new(APPLET, "owner contains NUL byte")])?;
    // SAFETY: `name` is a valid NUL-terminated username.
    let passwd = unsafe { libc::getpwnam(name.as_ptr()) };
    if passwd.is_null() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown user '{value}'"),
        )]);
    }
    // SAFETY: `passwd` is valid for the duration of this call.
    Ok(unsafe { ((*passwd).pw_uid, (*passwd).pw_gid) })
}

fn lookup_group(value: &str) -> Result<u32, Vec<AppletError>> {
    if let Ok(gid) = value.parse::<u32>() {
        return Ok(gid);
    }
    let name = CString::new(value)
        .map_err(|_| vec![AppletError::new(APPLET, "group contains NUL byte")])?;
    // SAFETY: `name` is a valid NUL-terminated group name.
    let group = unsafe { libc::getgrnam(name.as_ptr()) };
    if group.is_null() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown group '{value}'"),
        )]);
    }
    // SAFETY: `group` is valid for the duration of this call.
    Ok(unsafe { (*group).gr_gid })
}

fn create_archive(options: &Options) -> Result<(), Vec<AppletError>> {
    let names = read_input_names(options)?;
    let mut seen_links = HashMap::<(u64, u64), String>::new();
    let mut writer = open_output_writer(options).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            options.file.as_deref(),
            err,
        )]
    })?;

    for name in names {
        let archive_name = normalize_archive_name(&name)?;
        let metadata = metadata_for_source(Path::new(&name), options.dereference)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&name), err)])?;
        let entry = build_archive_entry(
            &name,
            &archive_name,
            &metadata,
            options.owner,
            options.dereference,
            &mut seen_links,
        )?;
        if options.verbose {
            eprintln!("{archive_name}");
        }
        write_entry(&mut writer, &entry).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "writing",
                options.file.as_deref(),
                err,
            )]
        })?;
    }

    let trailer = ArchiveEntry {
        ino: 0,
        mode: S_IFREG,
        uid: 0,
        gid: 0,
        nlink: 1,
        mtime: 0,
        dev_major: 0,
        dev_minor: 0,
        rdev_major: 0,
        rdev_minor: 0,
        name: TRAILER.to_string(),
        data: Vec::new(),
    };
    write_entry(&mut writer, &trailer).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "writing",
            options.file.as_deref(),
            err,
        )]
    })?;
    writer.flush().map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "writing",
            options.file.as_deref(),
            err,
        )]
    })?;
    Ok(())
}

fn pass_through(options: &Options, destination: &Path) -> Result<(), Vec<AppletError>> {
    let temp_path = std::env::temp_dir().join(format!(
        "seed-cpio-{}-{}.newc",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut temp_options = options.clone();
    temp_options.mode = Some(Mode::Create);
    temp_options.file = Some(temp_path.to_string_lossy().into_owned());
    temp_options.format_newc = true;
    create_archive(&temp_options)?;

    let mut extract_options = options.clone();
    extract_options.mode = Some(Mode::Extract);
    extract_options.file = Some(temp_path.to_string_lossy().into_owned());
    let result = extract_archive(&extract_options, destination);
    let _ = fs::remove_file(temp_path);
    result
}

fn list_archive(options: &Options) -> Result<(), Vec<AppletError>> {
    let mut reader = open_input_reader(options).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            options.file.as_deref(),
            err,
        )]
    })?;
    loop {
        let Some(entry) = read_entry(&mut reader)? else {
            break;
        };
        if entry.name == TRAILER {
            break;
        }
        if should_process_member(options, &entry.name) {
            println!("{}", entry.name);
        }
    }
    Ok(())
}

fn extract_archive(options: &Options, base: &Path) -> Result<(), Vec<AppletError>> {
    let mut reader = open_input_reader(options).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            options.file.as_deref(),
            err,
        )]
    })?;
    let mut hardlinks = HashMap::<(u32, u32, u32), PathBuf>::new();

    loop {
        let Some(entry) = read_entry(&mut reader)? else {
            break;
        };
        if entry.name == TRAILER {
            break;
        }
        if !should_process_member(options, &entry.name) {
            continue;
        }
        extract_entry(base, &entry, options, &mut hardlinks)?;
        if options.verbose {
            eprintln!("{}", entry.name);
        }
    }
    Ok(())
}

fn should_process_member(options: &Options, name: &str) -> bool {
    options.members.is_empty() || options.members.iter().any(|member| member == name)
}

fn read_input_names(options: &Options) -> Result<Vec<String>, Vec<AppletError>> {
    let mut reader = open_name_reader(options).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            options.file.as_deref(),
            err,
        )]
    })?;
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            options.file.as_deref(),
            err,
        )]
    })?;

    let separator = if options.nul_terminated { b'\0' } else { b'\n' };
    Ok(buffer
        .split(|byte| *byte == separator)
        .filter(|entry| !entry.is_empty())
        .map(|entry| String::from_utf8_lossy(entry).into_owned())
        .collect())
}

fn open_name_reader(options: &Options) -> io::Result<Box<dyn Read>> {
    let _ = options;
    Ok(Box::new(io::stdin()))
}

fn open_input_reader(options: &Options) -> io::Result<Box<dyn Read>> {
    if let Some(path) = options.file.as_deref()
        && path != "-"
    {
        return File::open(path).map(|file| Box::new(BufReader::new(file)) as Box<dyn Read>);
    }
    Ok(Box::new(io::stdin()))
}

fn open_output_writer(options: &Options) -> io::Result<Box<dyn Write>> {
    if let Some(path) = options.file.as_deref()
        && path != "-"
    {
        return OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)
            .map(|file| Box::new(BufWriter::new(file)) as Box<dyn Write>);
    }
    Ok(Box::new(BufWriter::new(io::stdout())))
}

fn normalize_archive_name(name: &str) -> Result<String, Vec<AppletError>> {
    let mut components = Vec::new();
    for component in Path::new(name).components() {
        match component {
            Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("refusing path with '..': {name}"),
                )]);
            }
            Component::Normal(part) => components.push(part.to_os_string()),
        }
    }

    if components.is_empty() {
        return Ok(String::from("."));
    }

    Ok(components
        .into_iter()
        .collect::<PathBuf>()
        .to_string_lossy()
        .replace('\\', "/"))
}

fn metadata_for_source(path: &Path, dereference: bool) -> io::Result<fs::Metadata> {
    if dereference {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    }
}

fn device_major(device: u64) -> u32 {
    libc::major(device as libc::dev_t) as u32
}

fn device_minor(device: u64) -> u32 {
    libc::minor(device as libc::dev_t) as u32
}

fn build_archive_entry(
    source_name: &str,
    archive_name: &str,
    metadata: &fs::Metadata,
    owner: Option<Owner>,
    dereference: bool,
    seen_links: &mut HashMap<(u64, u64), String>,
) -> Result<ArchiveEntry, Vec<AppletError>> {
    let file_type = metadata.file_type();
    let (uid, gid) = owner
        .map(|owner| (owner.uid, owner.gid))
        .unwrap_or((metadata.uid(), metadata.gid()));
    let key = (metadata.dev(), metadata.ino());
    let repeated_hardlink =
        metadata.nlink() > 1 && file_type.is_file() && seen_links.contains_key(&key);

    let data = if repeated_hardlink {
        Vec::new()
    } else if file_type.is_symlink() && !dereference {
        fs::read_link(source_name)
            .map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(source_name),
                    err,
                )]
            })?
            .as_os_str()
            .as_bytes()
            .to_vec()
    } else if file_type.is_file() {
        fs::read(source_name).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(source_name),
                err,
            )]
        })?
    } else {
        Vec::new()
    };

    if metadata.nlink() > 1 && file_type.is_file() && !seen_links.contains_key(&key) {
        seen_links.insert(key, archive_name.to_string());
    }

    Ok(ArchiveEntry {
        ino: metadata.ino() as u32,
        mode: metadata.mode(),
        uid,
        gid,
        nlink: metadata.nlink() as u32,
        mtime: metadata.mtime().max(0) as u32,
        dev_major: device_major(metadata.dev()),
        dev_minor: device_minor(metadata.dev()),
        rdev_major: device_major(metadata.rdev()),
        rdev_minor: device_minor(metadata.rdev()),
        name: archive_name.to_string(),
        data,
    })
}

fn write_entry(writer: &mut dyn Write, entry: &ArchiveEntry) -> io::Result<()> {
    let namesize = entry.name.len() + 1;
    let filesize = entry.data.len();
    write!(
        writer,
        "{NEWC_MAGIC}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
        entry.ino,
        entry.mode,
        entry.uid,
        entry.gid,
        entry.nlink,
        entry.mtime,
        filesize,
        entry.dev_major,
        entry.dev_minor,
        entry.rdev_major,
        entry.rdev_minor,
        namesize,
        0_u32
    )?;
    writer.write_all(entry.name.as_bytes())?;
    writer.write_all(&[0])?;
    write_padding(writer, 110 + namesize)?;
    writer.write_all(&entry.data)?;
    write_padding(writer, filesize)?;
    Ok(())
}

fn read_entry(reader: &mut dyn Read) -> Result<Option<ArchiveEntry>, Vec<AppletError>> {
    let mut header = [0_u8; 110];
    let mut read = 0;
    while read < header.len() {
        let count = reader
            .read(&mut header[read..])
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        if count == 0 {
            if read == 0 {
                return Ok(None);
            }
            return Err(vec![AppletError::new(APPLET, "short cpio header")]);
        }
        read += count;
    }

    let magic = std::str::from_utf8(&header[..6])
        .map_err(|_| vec![AppletError::new(APPLET, "invalid cpio magic")])?;
    if magic != "070701" && magic != "070702" {
        return Err(vec![AppletError::new(
            APPLET,
            "unsupported cpio format, use newc or crc",
        )]);
    }

    let fields = parse_header_fields(&header[6..])?;
    let namesize = fields[11] as usize;
    let filesize = fields[6] as usize;
    if namesize == 0 {
        return Err(vec![AppletError::new(APPLET, "invalid cpio member name")]);
    }

    let mut name = vec![0_u8; namesize];
    reader
        .read_exact(&mut name)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
    consume_padding(reader, 110 + namesize)?;

    let actual_name = String::from_utf8_lossy(&name[..namesize - 1]).into_owned();
    let mut data = vec![0_u8; filesize];
    reader.read_exact(&mut data).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(&actual_name),
            err,
        )]
    })?;
    consume_padding(reader, filesize)?;

    Ok(Some(ArchiveEntry {
        ino: fields[0],
        mode: fields[1],
        uid: fields[2],
        gid: fields[3],
        nlink: fields[4],
        mtime: fields[5],
        dev_major: fields[7],
        dev_minor: fields[8],
        rdev_major: fields[9],
        rdev_minor: fields[10],
        name: actual_name,
        data,
    }))
}

fn parse_header_fields(bytes: &[u8]) -> Result<[u32; 13], Vec<AppletError>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| vec![AppletError::new(APPLET, "invalid cpio header")])?;
    let mut fields = [0_u32; 13];
    for (index, field) in fields.iter_mut().enumerate() {
        let start = index * 8;
        let end = start + 8;
        *field = u32::from_str_radix(&text[start..end], 16)
            .map_err(|_| vec![AppletError::new(APPLET, "invalid cpio header")])?;
    }
    Ok(fields)
}

fn write_padding(writer: &mut dyn Write, written: usize) -> io::Result<()> {
    let pad = (4 - (written % 4)) % 4;
    if pad > 0 {
        writer.write_all(&[0_u8; 3][..pad])?;
    }
    Ok(())
}

fn consume_padding(reader: &mut dyn Read, written: usize) -> Result<(), Vec<AppletError>> {
    let pad = (4 - (written % 4)) % 4;
    if pad == 0 {
        return Ok(());
    }
    let mut discard = [0_u8; 3];
    reader
        .read_exact(&mut discard[..pad])
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
    Ok(())
}

fn extract_entry(
    base: &Path,
    entry: &ArchiveEntry,
    options: &Options,
    hardlinks: &mut HashMap<(u32, u32, u32), PathBuf>,
) -> Result<(), Vec<AppletError>> {
    let output = safe_output_path(base, &entry.name)?;
    let mode_kind = entry.mode & MODE_MASK;

    if entry.name == "." {
        if !base.exists() {
            fs::create_dir_all(base).map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "creating",
                    Some(&base.display().to_string()),
                    err,
                )]
            })?;
        }
        apply_metadata(base, entry, options, true)?;
        return Ok(());
    }

    ensure_parent_dirs(base, &output, options.make_dirs)?;
    remove_existing_if_needed(&output, options.overwrite, mode_kind == S_IFDIR)?;

    let link_key = (entry.dev_major, entry.dev_minor, entry.ino);
    match mode_kind {
        S_IFDIR => {
            fs::create_dir_all(&output).map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "creating",
                    Some(&entry.name),
                    err,
                )]
            })?;
            apply_metadata(&output, entry, options, true)?;
        }
        S_IFLNK => {
            let target = PathBuf::from(String::from_utf8_lossy(&entry.data).into_owned());
            symlink(&target, &output).map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "creating",
                    Some(&entry.name),
                    err,
                )]
            })?;
            apply_metadata(&output, entry, options, false)?;
        }
        S_IFREG if entry.nlink > 1 && entry.data.is_empty() => {
            if let Some(target) = hardlinks.get(&link_key) {
                fs::hard_link(target, &output).map_err(|err| {
                    vec![AppletError::from_io(
                        APPLET,
                        "creating",
                        Some(&entry.name),
                        err,
                    )]
                })?;
            } else {
                File::create(&output).map_err(|err| {
                    vec![AppletError::from_io(
                        APPLET,
                        "creating",
                        Some(&entry.name),
                        err,
                    )]
                })?;
                hardlinks.insert(link_key, output.clone());
            }
            apply_metadata(&output, entry, options, false)?;
        }
        S_IFREG => {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&output)
                .map_err(|err| {
                    vec![AppletError::from_io(
                        APPLET,
                        "creating",
                        Some(&entry.name),
                        err,
                    )]
                })?;
            file.write_all(&entry.data).map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "writing",
                    Some(&entry.name),
                    err,
                )]
            })?;
            hardlinks.insert(link_key, output.clone());
            apply_metadata(&output, entry, options, false)?;
        }
        S_IFIFO | S_IFCHR | S_IFBLK => {
            create_special_file(&output, entry)?;
            apply_metadata(&output, entry, options, false)?;
        }
        _ => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("unsupported file type in '{}'", entry.name),
            )]);
        }
    }

    Ok(())
}

fn safe_output_path(base: &Path, name: &str) -> Result<PathBuf, Vec<AppletError>> {
    let trimmed = name.trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(base.to_path_buf());
    }

    let mut relative = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("refusing unsafe path '{name}'"),
                )]);
            }
        }
    }
    Ok(base.join(relative))
}

fn ensure_parent_dirs(base: &Path, path: &Path, create_dirs: bool) -> Result<(), Vec<AppletError>> {
    let relative = path.strip_prefix(base).unwrap_or(path);
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    let mut current = base.to_path_buf();
    for component in parent.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("refusing to create through symlink '{}'", current.display()),
                )]);
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("parent is not a directory: '{}'", current.display()),
                )]);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound && create_dirs => {
                fs::create_dir(&current).map_err(|io_err| {
                    vec![AppletError::from_io(
                        APPLET,
                        "creating",
                        Some(&current.display().to_string()),
                        io_err,
                    )]
                })?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("missing parent directory for '{}'", path.display()),
                )]);
            }
            Err(err) => {
                return Err(vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(&current.display().to_string()),
                    err,
                )]);
            }
        }
    }
    Ok(())
}

fn remove_existing_if_needed(
    path: &Path,
    overwrite: bool,
    want_dir: bool,
) -> Result<(), Vec<AppletError>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.display().to_string()),
                err,
            )]);
        }
    };

    if want_dir && metadata.is_dir() && !metadata.file_type().is_symlink() {
        return Ok(());
    }
    if !overwrite {
        return Err(vec![AppletError::new(
            APPLET,
            format!("{} already exists", path.display()),
        )]);
    }

    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "removing",
                Some(&path.display().to_string()),
                err,
            )]
        })?;
    } else {
        fs::remove_file(path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "removing",
                Some(&path.display().to_string()),
                err,
            )]
        })?;
    }
    Ok(())
}

fn create_special_file(path: &Path, entry: &ArchiveEntry) -> Result<(), Vec<AppletError>> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let dev = libc::makedev(entry.rdev_major as _, entry.rdev_minor as _);
    // SAFETY: `c_path` is NUL-terminated and `mknod` is called with the mode and device from the archive header.
    let rc = unsafe { libc::mknod(c_path.as_ptr(), entry.mode as libc::mode_t, dev) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!(
                "creating {}: {}",
                path.display(),
                io::Error::last_os_error()
            ),
        )])
    }
}

fn apply_metadata(
    path: &Path,
    entry: &ArchiveEntry,
    options: &Options,
    is_dir: bool,
) -> Result<(), Vec<AppletError>> {
    if entry.mode & MODE_MASK != S_IFLNK {
        fs::set_permissions(path, fs::Permissions::from_mode(entry.mode & 0o7777)).map_err(
            |err| {
                vec![AppletError::from_io(
                    APPLET,
                    "chmod",
                    Some(&path.display().to_string()),
                    err,
                )]
            },
        )?;
    }

    if options.preserve_mtime {
        set_mtime(path, entry.mtime as i64, entry.mode & MODE_MASK == S_IFLNK)?;
    }

    if unsafe { libc::geteuid() } == 0 || options.owner.is_some() {
        let owner = options.owner.unwrap_or(Owner {
            uid: entry.uid,
            gid: entry.gid,
        });
        set_owner(path, owner, entry.mode & MODE_MASK == S_IFLNK)?;
    }

    if is_dir && options.preserve_mtime {
        set_mtime(path, entry.mtime as i64, false)?;
    }

    Ok(())
}

fn set_owner(path: &Path, owner: Owner, symlink_path: bool) -> Result<(), Vec<AppletError>> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    // SAFETY: `c_path` is NUL-terminated and the uid/gid values come from validated input or the archive.
    let rc = unsafe {
        if symlink_path {
            libc::lchown(c_path.as_ptr(), owner.uid, owner.gid)
        } else {
            libc::chown(c_path.as_ptr(), owner.uid, owner.gid)
        }
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("chown {}: {}", path.display(), io::Error::last_os_error()),
        )])
    }
}

fn set_mtime(path: &Path, seconds: i64, symlink_path: bool) -> Result<(), Vec<AppletError>> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let times = [
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: 0,
        },
    ];
    let flags = if symlink_path {
        libc::AT_SYMLINK_NOFOLLOW
    } else {
        0
    };
    // SAFETY: `c_path` is NUL-terminated and `times` points to two valid timestamps.
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!(
                "utimensat {}: {}",
                path.display(),
                io::Error::last_os_error()
            ),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, Options, parse_args, read_entry, write_entry};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_create_flags() {
        assert_eq!(
            parse_args(&args(&["-ovHnewc", "-F", "archive.cpio"])).unwrap(),
            Options {
                mode: Some(Mode::Create),
                file: Some("archive.cpio".to_string()),
                verbose: true,
                format_newc: true,
                ..Options::default()
            }
        );
    }

    #[test]
    fn parses_passthrough_with_combined_d_flag() {
        assert_eq!(
            parse_args(&args(&["-pd", "out"])).unwrap(),
            Options {
                mode: Some(Mode::PassThrough("out".into())),
                make_dirs: true,
                ..Options::default()
            }
        );
    }

    #[test]
    fn round_trips_newc_header() {
        let entry = super::ArchiveEntry {
            ino: 1,
            mode: super::S_IFREG | 0o644,
            uid: 1000,
            gid: 1000,
            nlink: 1,
            mtime: 7,
            dev_major: 0,
            dev_minor: 0,
            rdev_major: 0,
            rdev_minor: 0,
            name: "file".to_string(),
            data: b"hello".to_vec(),
        };
        let mut bytes = Vec::new();
        write_entry(&mut bytes, &entry).unwrap();
        let decoded = read_entry(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(decoded.name, "file");
        assert_eq!(decoded.data, b"hello");
        assert_eq!(decoded.mode & 0o777, 0o644);
    }
}
