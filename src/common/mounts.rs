use std::io;
use std::path::{Component, Path, PathBuf};

use std::fs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MountInfo {
    pub(crate) source: String,
    pub(crate) filesystem: String,
    pub(crate) mounted_on: String,
    pub(crate) options: Vec<String>,
}

pub(crate) fn mount_for_path(path: &Path) -> io::Result<Option<MountInfo>> {
    let target = fs::canonicalize(path)?;
    let mounts = read_mountinfo()?;
    Ok(mounts
        .into_iter()
        .filter(|mount| target.starts_with(Path::new(&mount.mounted_on)))
        .max_by_key(|mount| Path::new(&mount.mounted_on).components().count()))
}

pub(crate) fn mountpoint_at_path(path: &Path, nofollow: bool) -> io::Result<Option<MountInfo>> {
    let target = resolve_path(path, nofollow)?;
    let mounts = read_mountinfo()?;
    Ok(mounts
        .into_iter()
        .find(|mount| Path::new(&mount.mounted_on) == target))
}

pub(crate) fn format_device_number(dev: u64) -> String {
    let dev = dev as libc::dev_t;
    format!("{}:{}", libc::major(dev), libc::minor(dev))
}

pub(crate) fn read_mountinfo() -> io::Result<Vec<MountInfo>> {
    let path = std::env::var_os("SEED_MOUNTINFO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc/self/mountinfo"));
    let mountinfo = fs::read_to_string(path)?;
    Ok(mountinfo.lines().filter_map(parse_mountinfo_line).collect())
}

fn parse_mountinfo_line(line: &str) -> Option<MountInfo> {
    let (left, right) = line.split_once(" - ")?;
    let left_fields = left.split_whitespace().collect::<Vec<_>>();
    let right_fields = right.split_whitespace().collect::<Vec<_>>();
    if left_fields.len() < 5 || right_fields.len() < 2 {
        return None;
    }

    Some(MountInfo {
        source: unescape_mount_field(right_fields[1]),
        filesystem: unescape_mount_field(right_fields[0]),
        mounted_on: unescape_mount_field(left_fields[4]),
        options: left_fields[5]
            .split(',')
            .filter(|option| !option.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

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

fn resolve_path(path: &Path, nofollow: bool) -> io::Result<PathBuf> {
    if !nofollow {
        return fs::canonicalize(path);
    }

    let mut absolute = if path.is_absolute() {
        PathBuf::from("/")
    } else {
        std::env::current_dir()?
    };

    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                absolute.pop();
            }
            Component::Normal(part) => absolute.push(part),
            Component::Prefix(_) => {}
        }
    }

    Ok(absolute)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{format_device_number, mountpoint_at_path, parse_mountinfo_line};
    #[cfg(target_os = "linux")]
    use std::path::Path;

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_mountinfo_lines() {
        let mount = parse_mountinfo_line(
            "26 20 0:21 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw",
        )
        .expect("parse mountinfo");
        assert_eq!(mount.source, "proc");
        assert_eq!(mount.filesystem, "proc");
        assert_eq!(mount.mounted_on, "/proc");
        assert!(mount.options.iter().any(|option| option == "rw"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn formats_device_numbers() {
        assert!(!format_device_number(0).is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn finds_proc_as_mountpoint() {
        let mount = mountpoint_at_path(Path::new("/proc"), false).expect("mountpoint lookup");
        assert!(mount.is_some());
    }
}
