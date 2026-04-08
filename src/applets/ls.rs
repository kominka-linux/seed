use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "ls";
const SIX_MONTHS_SECONDS: i64 = 15_552_000;

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    single_column: bool,
    human_readable: bool,
    long_format: bool,
    show_blocks: bool,
}

#[derive(Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    metadata: fs::Metadata,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, paths) = parse_args(args)?;
    let targets = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    let mut output = String::new();
    let mut errors = Vec::new();

    for path in &targets {
        match render_target(path, options) {
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

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut paths = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    '1' => options.single_column = true,
                    'h' => options.human_readable = true,
                    'l' => options.long_format = true,
                    's' => options.show_blocks = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        paths.push(arg.clone());
    }

    Ok((options, paths))
}

fn render_target(path: &str, options: Options) -> Result<String, AppletError> {
    let path_ref = Path::new(path);
    let metadata = fs::symlink_metadata(path_ref)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?;

    if should_list_directory(path_ref, &metadata) {
        render_directory(path_ref, options)
    } else {
        render_file(path, path_ref, metadata, options)
    }
}

fn should_list_directory(path: &Path, metadata: &fs::Metadata) -> bool {
    metadata.is_dir()
        || (metadata.file_type().is_symlink()
            && fs::metadata(path).is_ok_and(|target| target.is_dir()))
}

fn render_directory(path: &Path, options: Options) -> Result<String, AppletError> {
    let display = path.to_string_lossy();
    let mut entries = Vec::new();

    for entry in fs::read_dir(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?
    {
        let entry =
            entry.map_err(|err| AppletError::from_io(APPLET, "reading", Some(&display), err))?;
        let name = entry.file_name();
        let name = String::from_utf8_lossy(name.as_os_str().as_bytes()).into_owned();
        let child_path = entry.path();
        let metadata = fs::symlink_metadata(&child_path)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(&name), err))?;
        entries.push(Entry {
            name,
            path: child_path,
            metadata,
        });
    }

    entries.sort_by(|left, right| left.name.as_bytes().cmp(right.name.as_bytes()));

    render_entries(&entries, options, true)
}

fn render_file(
    display: &str,
    path: &Path,
    metadata: fs::Metadata,
    options: Options,
) -> Result<String, AppletError> {
    let entry = Entry {
        name: display.to_owned(),
        path: path.to_path_buf(),
        metadata,
    };
    render_entries(&[entry], options, false)
}

fn render_entries(
    entries: &[Entry],
    options: Options,
    show_total: bool,
) -> Result<String, AppletError> {
    let mut output = String::new();

    if show_total && (options.long_format || options.show_blocks) {
        output.push_str(&format!("total {}\n", total_blocks(entries)));
    }

    for entry in entries {
        if options.long_format {
            output.push_str(&format_long_entry(entry, options)?);
        } else if options.show_blocks {
            output.push_str(&format!(
                "{} {}\n",
                format_blocks(entry.metadata.blocks(), options.human_readable),
                entry.name
            ));
        } else {
            let _ = options.single_column;
            output.push_str(&entry.name);
            output.push('\n');
        }
    }

    Ok(output)
}

fn total_blocks(entries: &[Entry]) -> u64 {
    entries.iter().map(|entry| entry.metadata.blocks()).sum()
}

fn format_long_entry(entry: &Entry, options: Options) -> Result<String, AppletError> {
    let mode = format_mode(entry)?;
    let links = entry.metadata.nlink();
    let owner = lookup_user(entry.metadata.uid());
    let group = lookup_group(entry.metadata.gid());
    let size = if options.human_readable {
        format_human_size(entry.metadata.size())
    } else {
        entry.metadata.size().to_string()
    };
    let modified = format_mtime(entry.metadata.mtime())?;
    let mut line = format!(
        "{mode} {} {} {} {} {} {}",
        links, owner, group, size, modified, entry.name
    );
    if entry.metadata.file_type().is_symlink()
        && let Ok(target) = fs::read_link(&entry.path)
    {
        line.push_str(" -> ");
        line.push_str(&target.to_string_lossy());
    }
    line.push('\n');
    Ok(line)
}

fn format_mode(entry: &Entry) -> Result<String, AppletError> {
    let mode = entry.metadata.mode();
    let mut text = String::with_capacity(11);
    text.push(file_type_char(entry.metadata.file_type()));
    text.push(permission_char(mode, 0o400, b'r'));
    text.push(permission_char(mode, 0o200, b'w'));
    text.push(execute_char(mode, 0o100, 0o4000, b'x', b's', b'S'));
    text.push(permission_char(mode, 0o040, b'r'));
    text.push(permission_char(mode, 0o020, b'w'));
    text.push(execute_char(mode, 0o010, 0o2000, b'x', b's', b'S'));
    text.push(permission_char(mode, 0o004, b'r'));
    text.push(permission_char(mode, 0o002, b'w'));
    text.push(execute_char(mode, 0o001, 0o1000, b'x', b't', b'T'));
    text.push(attribute_marker(&entry.path)?);
    Ok(text)
}

fn file_type_char(file_type: fs::FileType) -> char {
    if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_block_device() {
        'b'
    } else if file_type.is_char_device() {
        'c'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else {
        '-'
    }
}

fn permission_char(mode: u32, bit: u32, set: u8) -> char {
    if mode & bit != 0 {
        char::from(set)
    } else {
        '-'
    }
}

fn execute_char(
    mode: u32,
    execute_bit: u32,
    special_bit: u32,
    execute: u8,
    special: u8,
    plain_special: u8,
) -> char {
    match (mode & execute_bit != 0, mode & special_bit != 0) {
        (true, true) => char::from(special),
        (true, false) => char::from(execute),
        (false, true) => char::from(plain_special),
        (false, false) => '-',
    }
}

fn attribute_marker(path: &Path) -> Result<char, AppletError> {
    let bytes = path.as_os_str().as_bytes();
    let path_c = CString::new(bytes)
        .map_err(|_| AppletError::new(APPLET, format!("unsupported path '{}'", path.display())))?;
    #[cfg(target_os = "macos")]
    // SAFETY: `path_c` is a valid NUL-terminated path string. Passing a null
    // buffer with size 0 asks `listxattr` for the required size only.
    let size = unsafe { libc::listxattr(path_c.as_ptr(), std::ptr::null_mut(), 0, 0) };
    #[cfg(not(target_os = "macos"))]
    // SAFETY: `path_c` is a valid NUL-terminated path string. Passing a null
    // buffer with size 0 asks `listxattr` for the required size only.
    let size = unsafe { libc::listxattr(path_c.as_ptr(), std::ptr::null_mut(), 0) };
    if size > 0 { Ok('@') } else { Ok(' ') }
}

fn lookup_user(uid: u32) -> String {
    // SAFETY: `getpwuid` returns either null or a pointer to static storage
    // owned by libc for the current process. We copy the resulting name
    // immediately into an owned Rust `String`.
    let passwd = unsafe { libc::getpwuid(uid) };
    if passwd.is_null() {
        return uid.to_string();
    }
    // SAFETY: `passwd` was checked for null and points to a valid `passwd`
    // record for the lifetime of this call.
    unsafe { CStr::from_ptr((*passwd).pw_name) }
        .to_string_lossy()
        .into_owned()
}

fn lookup_group(gid: u32) -> String {
    // SAFETY: `getgrgid` returns either null or a pointer to static storage
    // owned by libc for the current process. We copy the resulting name
    // immediately into an owned Rust `String`.
    let group = unsafe { libc::getgrgid(gid) };
    if group.is_null() {
        return gid.to_string();
    }
    // SAFETY: `group` was checked for null and points to a valid `group`
    // record for the lifetime of this call.
    unsafe { CStr::from_ptr((*group).gr_name) }
        .to_string_lossy()
        .into_owned()
}

fn format_mtime(seconds: i64) -> Result<String, AppletError> {
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let mut now = 0 as libc::time_t;
    // SAFETY: libc initializes `now` and `tm` for valid input pointers.
    unsafe {
        libc::time(&mut now);
        if libc::localtime_r(&(seconds as libc::time_t), tm.as_mut_ptr()).is_null() {
            return Err(AppletError::new(APPLET, "failed to format file time"));
        }
    }

    let recent = (seconds - now).abs() <= SIX_MONTHS_SECONDS;
    let format = if recent { "%b %e %H:%M" } else { "%b %e  %Y" };
    let format_c = CString::new(format).expect("strftime formats are NUL-free");
    let mut buf = [0_u8; 64];
    // SAFETY: `tm` was initialized by `localtime_r`, `buf` is valid writable
    // memory, and `format_c` is a valid NUL-terminated format string.
    let written = unsafe {
        libc::strftime(
            buf.as_mut_ptr().cast(),
            buf.len(),
            format_c.as_ptr(),
            tm.as_ptr(),
        )
    };
    if written == 0 {
        return Err(AppletError::new(APPLET, "failed to format file time"));
    }
    Ok(String::from_utf8_lossy(&buf[..written]).into_owned())
}

fn format_blocks(blocks: u64, human_readable: bool) -> String {
    if human_readable {
        format_human_size(blocks.saturating_mul(512))
    } else {
        blocks.to_string()
    }
}

fn format_human_size(size: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        size.to_string()
    } else if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, render_target};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let mut path = std::env::temp_dir();
            let unique = format!(
                "seed-ls-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock drift")
                    .as_nanos()
            );
            path.push(unique);
            fs::create_dir(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn long_listing_for_file_operand_does_not_print_total() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                long_format: true,
                ..Options::default()
            },
        )
        .expect("render long file");

        assert!(!output.starts_with("total "));
    }

    #[test]
    fn block_listing_for_file_operand_does_not_print_total() {
        let tempdir = TempDir::new();
        let file = tempdir.path().join("file");
        fs::write(&file, b"x").expect("write file");

        let output = render_target(
            file.to_str().expect("utf8 path"),
            Options {
                show_blocks: true,
                ..Options::default()
            },
        )
        .expect("render block file");

        assert!(!output.starts_with("total "));
    }
}
