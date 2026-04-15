use std::ffi::{CStr, CString};
use std::mem::MaybeUninit;
use std::os::raw::c_char;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use crate::common::error::AppletError;

const SIX_MONTHS_SECONDS: i64 = 15_552_000;

#[derive(Clone, Copy, Debug)]
pub(crate) enum FileKind {
    Regular,
    Directory,
    Symlink,
    BlockDevice,
    CharDevice,
    Fifo,
    Socket,
}

pub(crate) fn format_mode(kind: FileKind, mode: u32, trailing: Option<char>) -> String {
    let mut text = String::with_capacity(if trailing.is_some() { 11 } else { 10 });
    text.push(file_type_char(kind));
    text.push(permission_char(mode, 0o400, b'r'));
    text.push(permission_char(mode, 0o200, b'w'));
    text.push(execute_char(mode, 0o100, 0o4000, b'x', b's', b'S'));
    text.push(permission_char(mode, 0o040, b'r'));
    text.push(permission_char(mode, 0o020, b'w'));
    text.push(execute_char(mode, 0o010, 0o2000, b'x', b's', b'S'));
    text.push(permission_char(mode, 0o004, b'r'));
    text.push(permission_char(mode, 0o002, b'w'));
    text.push(execute_char(mode, 0o001, 0o1000, b'x', b't', b'T'));
    if let Some(marker) = trailing {
        text.push(marker);
    }
    text
}

pub(crate) fn lookup_user(uid: u32) -> String {
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

pub(crate) fn lookup_group(gid: u32) -> String {
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

pub(crate) fn format_recent_mtime(
    applet: &'static str,
    seconds: i64,
) -> Result<String, AppletError> {
    let mut now: libc::c_long = 0;
    // SAFETY: libc initializes `now` for a valid input pointer.
    unsafe {
        libc::time(&mut now);
    }

    let format = if (seconds - now as i64).abs() <= SIX_MONTHS_SECONDS {
        "%b %e %H:%M"
    } else {
        "%b %e  %Y"
    };
    format_local_time(applet, seconds, format)
}

pub(crate) fn format_full_timestamp(
    applet: &'static str,
    seconds: u64,
) -> Result<String, AppletError> {
    let seconds =
        i64::try_from(seconds).map_err(|_| AppletError::new(applet, "timestamp out of range"))?;
    format_local_time(applet, seconds, "%Y-%m-%d %H:%M:%S")
}

fn file_type_char(kind: FileKind) -> char {
    match kind {
        FileKind::Regular => '-',
        FileKind::Directory => 'd',
        FileKind::Symlink => 'l',
        FileKind::BlockDevice => 'b',
        FileKind::CharDevice => 'c',
        FileKind::Fifo => 'p',
        FileKind::Socket => 's',
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

fn format_local_time(
    applet: &'static str,
    seconds: i64,
    format: &str,
) -> Result<String, AppletError> {
    #[allow(clippy::useless_conversion)]
    let c_seconds: libc::c_long = libc::c_long::try_from(seconds)
        .map_err(|_| AppletError::new(applet, "timestamp out of range"))?;
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    let format_c = CString::new(format).expect("strftime formats are NUL-free");
    let mut buffer = [0 as c_char; 64];

    // SAFETY: `seconds`, `tm`, `buffer`, and `format_c` all point to valid
    // memory for the duration of the call, and `format_c` is NUL-terminated.
    let written = unsafe {
        if libc::localtime_r(&c_seconds, tm.as_mut_ptr()).is_null() {
            return Err(AppletError::new(applet, "failed to format file time"));
        }
        libc::strftime(
            buffer.as_mut_ptr(),
            buffer.len(),
            format_c.as_ptr(),
            tm.as_ptr(),
        )
    };
    if written == 0 {
        return Err(AppletError::new(applet, "failed to format file time"));
    }

    let bytes = buffer[..written]
        .iter()
        .map(|&byte| byte as u8)
        .collect::<Vec<_>>();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
pub(crate) fn temp_dir(label: &str) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("seed-{label}-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
