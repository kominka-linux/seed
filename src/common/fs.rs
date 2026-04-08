use std::collections::HashMap;
use std::ffi::CString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use crate::common::io::{BUFFER_SIZE, copy_stream};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dereference {
    Never,
    CommandLine,
    Always,
}

#[derive(Clone, Copy, Debug)]
pub struct CopyOptions {
    pub recursive: bool,
    pub dereference: Dereference,
    pub preserve_hard_links: bool,
    pub preserve_mode: bool,
    pub preserve_timestamps: bool,
}

#[derive(Debug)]
pub enum CopyError {
    Io(io::Error),
    OmitDirectory,
}

pub struct CopyContext {
    hard_links: HashMap<(u64, u64), PathBuf>,
}

impl CopyContext {
    pub fn new() -> Self {
        Self {
            hard_links: HashMap::new(),
        }
    }
}

impl Default for CopyContext {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_path(
    src: &Path,
    dest: &Path,
    options: &CopyOptions,
    context: &mut CopyContext,
    top_level: bool,
) -> Result<(), CopyError> {
    let source_meta = fs::symlink_metadata(src).map_err(CopyError::Io)?;

    if source_meta.file_type().is_symlink() && !should_follow(options.dereference, top_level) {
        return copy_symlink(src, dest);
    }

    let effective_meta = if source_meta.file_type().is_symlink() {
        fs::metadata(src).map_err(CopyError::Io)?
    } else {
        source_meta
    };

    if effective_meta.is_dir() {
        return copy_directory(src, dest, &effective_meta, options, context);
    }

    copy_streaming_file(src, dest, &effective_meta, options, context)
}

pub fn move_path(src: &Path, dest: &Path) -> io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(libc::EXDEV) => {
            let mut context = CopyContext::new();
            let options = CopyOptions {
                recursive: true,
                dereference: Dereference::Never,
                preserve_hard_links: true,
                preserve_mode: true,
                preserve_timestamps: true,
            };
            copy_path(src, dest, &options, &mut context, true).map_err(|err| match err {
                CopyError::Io(err) => err,
                CopyError::OmitDirectory => io::Error::other("unexpected directory omission"),
            })?;
            remove_path(src, true, false)
        }
        Err(err) => Err(err),
    }
}

pub fn remove_path(path: &Path, recursive: bool, force: bool) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if force && err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        if recursive {
            fs::remove_dir_all(path)
        } else {
            fs::remove_dir(path)
        }
    } else {
        fs::remove_file(path)
    }
}

fn should_follow(mode: Dereference, top_level: bool) -> bool {
    match mode {
        Dereference::Never => false,
        Dereference::CommandLine => top_level,
        Dereference::Always => true,
    }
}

fn copy_symlink(src: &Path, dest: &Path) -> Result<(), CopyError> {
    let target = fs::read_link(src).map_err(CopyError::Io)?;
    ensure_parent(dest).map_err(CopyError::Io)?;
    remove_existing_file_like(dest).map_err(CopyError::Io)?;
    symlink(target, dest).map_err(CopyError::Io)
}

fn copy_directory(
    src: &Path,
    dest: &Path,
    metadata: &Metadata,
    options: &CopyOptions,
    context: &mut CopyContext,
) -> Result<(), CopyError> {
    if !options.recursive {
        return Err(CopyError::OmitDirectory);
    }

    ensure_parent(dest).map_err(CopyError::Io)?;
    match fs::create_dir(dest) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(CopyError::Io(err)),
    }

    for entry in fs::read_dir(src).map_err(CopyError::Io)? {
        let entry = entry.map_err(CopyError::Io)?;
        let child_dest = dest.join(entry.file_name());
        copy_path(&entry.path(), &child_dest, options, context, false)?;
    }

    if options.preserve_mode {
        fs::set_permissions(dest, fs::Permissions::from_mode(metadata.mode() & 0o7777))
            .map_err(CopyError::Io)?;
    }

    Ok(())
}

fn copy_streaming_file(
    src: &Path,
    dest: &Path,
    metadata: &Metadata,
    options: &CopyOptions,
    context: &mut CopyContext,
) -> Result<(), CopyError> {
    ensure_parent(dest).map_err(CopyError::Io)?;

    if options.preserve_hard_links && metadata.nlink() > 1 {
        let key = (metadata.dev(), metadata.ino());
        if let Some(previous) = context.hard_links.get(&key) {
            remove_existing_file_like(dest).map_err(CopyError::Io)?;
            fs::hard_link(previous, dest).map_err(CopyError::Io)?;
            return Ok(());
        }
    }

    let source = File::open(src).map_err(CopyError::Io)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, source);

    remove_existing_file_like(dest).map_err(CopyError::Io)?;
    let destination = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(dest)
        .map_err(CopyError::Io)?;
    let mut writer = BufWriter::with_capacity(BUFFER_SIZE, destination);
    copy_stream(&mut reader, &mut writer).map_err(CopyError::Io)?;
    writer.flush().map_err(CopyError::Io)?;

    if options.preserve_mode {
        fs::set_permissions(dest, fs::Permissions::from_mode(metadata.mode() & 0o7777))
            .map_err(CopyError::Io)?;
    }
    if options.preserve_timestamps {
        set_timestamps(dest, metadata).map_err(CopyError::Io)?;
    }
    if options.preserve_hard_links && metadata.nlink() > 1 {
        context
            .hard_links
            .insert((metadata.dev(), metadata.ino()), dest.to_path_buf());
    }

    Ok(())
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn remove_existing_file_like(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("{} is a directory", path.display()),
            ))
        }
        Ok(_) => fs::remove_file(path),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn set_timestamps(path: &Path, metadata: &Metadata) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))?;
    let times = [
        libc::timespec {
            tv_sec: metadata.atime(),
            tv_nsec: metadata.atime_nsec(),
        },
        libc::timespec {
            tv_sec: metadata.mtime(),
            tv_nsec: metadata.mtime_nsec(),
        },
    ];

    // SAFETY: `path` is a valid NUL-terminated path string and `times` points to
    // two initialized `timespec` values for access and modification times.
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
