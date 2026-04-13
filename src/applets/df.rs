use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[cfg(target_os = "linux")]
use std::fs;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "df";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let paths = parse_args(args)?;
    let paths = if paths.is_empty() {
        vec![String::from(".")]
    } else {
        paths
    };

    println!("Filesystem 1024-blocks Used Available Capacity Mounted on");
    let mut errors = Vec::new();

    for path in &paths {
        match stat_filesystem(path) {
            Ok(stats) => println!(
                "{} {} {} {} {} {}",
                stats.filesystem,
                stats.blocks,
                stats.used,
                stats.available,
                stats.capacity,
                stats.mounted_on
            ),
            Err(err) => errors.push(err),
        }
    }

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
        if parsing_flags && (arg == "-k" || arg == "-P" || arg == "-kP" || arg == "-Pk") {
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

struct FsStats {
    filesystem: String,
    blocks: u64,
    used: u64,
    available: u64,
    capacity: String,
    mounted_on: String,
}

fn stat_filesystem(path: &str) -> Result<FsStats, AppletError> {
    let c_path = CString::new(Path::new(path).as_os_str().as_bytes())
        .map_err(|_| AppletError::new(APPLET, "path contains NUL byte"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: pointers are valid and `stats` is writable for libc.
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if rc != 0 {
        return Err(AppletError::from_io(
            APPLET,
            "reading",
            Some(path),
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: libc initialized `stats` on success.
    let stats = unsafe { stats.assume_init() };

    let block_size = stats.f_bsize as u64;
    let blocks = stats.f_blocks * block_size / 1024;
    let used = (stats.f_blocks - stats.f_bfree) * block_size / 1024;
    let available = stats.f_bavail * block_size / 1024;
    let capacity = if used + available == 0 {
        "0%".to_string()
    } else {
        format!("{}%", (used * 100).div_ceil(used + available))
    };
    let mount = mount_for_path(path)?;

    Ok(FsStats {
        filesystem: mount.filesystem,
        blocks,
        used,
        available,
        capacity,
        mounted_on: mount.mounted_on,
    })
}

struct MountInfo {
    filesystem: String,
    mounted_on: String,
}

fn mount_for_path(path: &str) -> Result<MountInfo, AppletError> {
    #[cfg(target_os = "linux")]
    {
        return mount_for_path_linux(path);
    }

    #[cfg(target_os = "macos")]
    {
        return mount_for_path_macos(path);
    }

    #[allow(unreachable_code)]
    Err(AppletError::new(APPLET, "unsupported on this platform"))
}

#[cfg(target_os = "linux")]
fn mount_for_path_linux(path: &str) -> Result<MountInfo, AppletError> {
    let target = fs::canonicalize(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?;
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").map_err(|err| {
        AppletError::from_io(APPLET, "reading", Some("/proc/self/mountinfo"), err)
    })?;

    mountinfo
        .lines()
        .filter_map(parse_mountinfo_line)
        .filter(|mount| target.starts_with(Path::new(&mount.mounted_on)))
        .max_by_key(|mount| Path::new(&mount.mounted_on).components().count())
        .ok_or_else(|| AppletError::new(APPLET, format!("no mount for '{path}'")))
}

#[cfg(target_os = "linux")]
fn parse_mountinfo_line(line: &str) -> Option<MountInfo> {
    let (left, right) = line.split_once(" - ")?;
    let left_fields = left.split_whitespace().collect::<Vec<_>>();
    let right_fields = right.split_whitespace().collect::<Vec<_>>();
    if left_fields.len() < 5 || right_fields.len() < 2 {
        return None;
    }

    Some(MountInfo {
        filesystem: unescape_mount_field(right_fields[1]),
        mounted_on: unescape_mount_field(left_fields[4]),
    })
}

#[cfg(target_os = "linux")]
fn unescape_mount_field(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &value[index + 1..index + 4];
            if octal
                .as_bytes()
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
                && let Ok(decoded) = u8::from_str_radix(octal, 8)
            {
                result.push(decoded as char);
                index += 4;
                continue;
            }
        }
        result.push(bytes[index] as char);
        index += 1;
    }
    result
}

#[cfg(target_os = "macos")]
fn mount_for_path_macos(path: &str) -> Result<MountInfo, AppletError> {
    let c_path = CString::new(Path::new(path).as_os_str().as_bytes())
        .map_err(|_| AppletError::new(APPLET, "path contains NUL byte"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: pointers are valid and `stats` is writable for libc.
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if rc != 0 {
        return Err(AppletError::from_io(
            APPLET,
            "reading",
            Some(path),
            std::io::Error::last_os_error(),
        ));
    }
    // SAFETY: libc initialized `stats` on success.
    let stats = unsafe { stats.assume_init() };
    Ok(MountInfo {
        filesystem: c_buf_to_string(&stats.f_mntfromname),
        mounted_on: c_buf_to_string(&stats.f_mntonname),
    })
}

#[cfg(target_os = "macos")]
fn c_buf_to_string(buf: &[libc::c_char]) -> String {
    let bytes = buf
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn accepts_kp_flags() {
        assert_eq!(parse_args(&args(&["-kP", "."])).unwrap(), vec!["."]);
        assert_eq!(parse_args(&args(&["-P", "-k", "."])).unwrap(), vec!["."]);
    }
}
