use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AttributeFlag {
    pub(crate) short: char,
    pub(crate) bit: u32,
    pub(crate) long: &'static str,
}

pub(crate) const DIRSYNC: u32 = 0x0001_0000;

pub(crate) const ATTRIBUTE_FLAGS: [AttributeFlag; 13] = [
    AttributeFlag {
        short: 'I',
        bit: 0x0000_1000,
        long: "Indexed_directory",
    },
    AttributeFlag {
        short: 's',
        bit: 0x0000_0001,
        long: "Secure_Deletion",
    },
    AttributeFlag {
        short: 'u',
        bit: 0x0000_0002,
        long: "Undelete",
    },
    AttributeFlag {
        short: 'S',
        bit: 0x0000_0008,
        long: "Synchronous_Updates",
    },
    AttributeFlag {
        short: 'D',
        bit: DIRSYNC,
        long: "Synchronous_Directory_Updates",
    },
    AttributeFlag {
        short: 'i',
        bit: 0x0000_0010,
        long: "Immutable",
    },
    AttributeFlag {
        short: 'a',
        bit: 0x0000_0020,
        long: "Append_Only",
    },
    AttributeFlag {
        short: 'd',
        bit: 0x0000_0040,
        long: "No_Dump",
    },
    AttributeFlag {
        short: 'A',
        bit: 0x0000_0080,
        long: "No_Atime",
    },
    AttributeFlag {
        short: 'c',
        bit: 0x0000_0004,
        long: "Compression_Requested",
    },
    AttributeFlag {
        short: 'j',
        bit: 0x0000_4000,
        long: "Journaled_Data",
    },
    AttributeFlag {
        short: 't',
        bit: 0x0000_8000,
        long: "No_Tailmerging",
    },
    AttributeFlag {
        short: 'T',
        bit: 0x0002_0000,
        long: "Top_of_Directory_Hierarchies",
    },
];

#[derive(Clone, Copy, Debug, Default)]
struct StateEntry {
    flags: u32,
    version: u32,
}

pub(crate) fn parse_flag_mask(spec: &str) -> io::Result<u32> {
    let mut mask = 0;
    for flag in spec.chars() {
        let Some(entry) = ATTRIBUTE_FLAGS.iter().find(|entry| entry.short == flag) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported attribute '{flag}'"),
            ));
        };
        mask |= entry.bit;
    }
    Ok(mask)
}

pub(crate) fn format_short_flags(flags: u32) -> String {
    ATTRIBUTE_FLAGS
        .iter()
        .map(|entry| {
            if flags & entry.bit != 0 {
                entry.short
            } else {
                '-'
            }
        })
        .collect()
}

pub(crate) fn format_long_flags(flags: u32) -> String {
    let active = ATTRIBUTE_FLAGS
        .iter()
        .filter(|entry| flags & entry.bit != 0)
        .map(|entry| entry.long)
        .collect::<Vec<_>>();
    if active.is_empty() {
        String::from("---")
    } else {
        active.join(", ")
    }
}

pub(crate) fn get_flags(path: &Path) -> io::Result<u32> {
    if let Some(state_path) = state_path() {
        return Ok(read_state(&state_path)?
            .get(&state_key(path))
            .map_or(0, |entry| entry.flags));
    }

    #[cfg(target_os = "linux")]
    {
        return ioctl_get_u32(path, fs_ioc_getflags());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported on this platform",
        ))
    }
}

pub(crate) fn set_flags(path: &Path, flags: u32) -> io::Result<()> {
    if let Some(state_path) = state_path() {
        let mut state = read_state(&state_path)?;
        state.entry(state_key(path)).or_default().flags = flags;
        return write_state(&state_path, &state);
    }

    #[cfg(target_os = "linux")]
    {
        return ioctl_set_u32(path, fs_ioc_setflags(), flags);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, flags);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported on this platform",
        ))
    }
}

pub(crate) fn get_version(path: &Path) -> io::Result<u32> {
    if let Some(state_path) = state_path() {
        return Ok(read_state(&state_path)?
            .get(&state_key(path))
            .map_or(0, |entry| entry.version));
    }

    #[cfg(target_os = "linux")]
    {
        return ioctl_get_u32(path, fs_ioc_getversion());
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported on this platform",
        ))
    }
}

pub(crate) fn set_version(path: &Path, version: u32) -> io::Result<()> {
    if let Some(state_path) = state_path() {
        let mut state = read_state(&state_path)?;
        state.entry(state_key(path)).or_default().version = version;
        return write_state(&state_path, &state);
    }

    #[cfg(target_os = "linux")]
    {
        return ioctl_set_u32(path, fs_ioc_setversion(), version);
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, version);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported on this platform",
        ))
    }
}

fn state_path() -> Option<PathBuf> {
    env::var_os("SEED_EXTATTR_STATE").map(PathBuf::from)
}

fn state_key(path: &Path) -> String {
    path.as_os_str().to_string_lossy().into_owned()
}

fn read_state(path: &Path) -> io::Result<BTreeMap<String, StateEntry>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => return Err(err),
    };

    let mut state = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.splitn(3, '\t');
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(flags) = fields.next() else {
            continue;
        };
        let Some(version) = fields.next() else {
            continue;
        };
        let Ok(flags) = flags.parse::<u32>() else {
            continue;
        };
        let Ok(version) = version.parse::<u32>() else {
            continue;
        };
        state.insert(name.to_string(), StateEntry { flags, version });
    }
    Ok(state)
}

fn write_state(path: &Path, state: &BTreeMap<String, StateEntry>) -> io::Result<()> {
    let mut text = String::new();
    for (name, entry) in state {
        text.push_str(name);
        text.push('\t');
        text.push_str(&entry.flags.to_string());
        text.push('\t');
        text.push_str(&entry.version.to_string());
        text.push('\n');
    }
    fs::write(path, text)
}

#[cfg(target_os = "linux")]
fn ioctl_get_u32(path: &Path, request: libc::c_ulong) -> io::Result<u32> {
    let fd = open_path(path)?;
    let mut value = 0u32;
    // SAFETY: `fd` is an open descriptor for `path`, and `value` points to writable storage.
    let rc = unsafe { libc::ioctl(fd, request as _, &mut value) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(value) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn ioctl_set_u32(path: &Path, request: libc::c_ulong, value: u32) -> io::Result<()> {
    let fd = open_path(path)?;
    let mut value = value;
    // SAFETY: `fd` is an open descriptor for `path`, and `value` points to readable storage.
    let rc = unsafe { libc::ioctl(fd, request as _, &mut value) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn open_path(path: &Path) -> io::Result<libc::c_int> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))?;
    // SAFETY: `c_path` is a valid NUL-terminated pathname.
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn close_fd(fd: libc::c_int) {
    // SAFETY: `fd` is either valid or already rejected by callers; close errors are intentionally ignored.
    unsafe {
        libc::close(fd);
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc", target_arch = "mips")
))]
const fn fs_ioc_getflags() -> libc::c_ulong {
    0x4004_6601
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc", target_arch = "mips")
))]
const fn fs_ioc_setflags() -> libc::c_ulong {
    0x8004_6602
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc", target_arch = "mips")
))]
const fn fs_ioc_getversion() -> libc::c_ulong {
    0x4004_7601
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc", target_arch = "mips")
))]
const fn fs_ioc_setversion() -> libc::c_ulong {
    0x8004_7602
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc64", target_arch = "mips64")
))]
const fn fs_ioc_getflags() -> libc::c_ulong {
    0x4008_6601
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc64", target_arch = "mips64")
))]
const fn fs_ioc_setflags() -> libc::c_ulong {
    0x8008_6602
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc64", target_arch = "mips64")
))]
const fn fs_ioc_getversion() -> libc::c_ulong {
    0x4008_7601
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "powerpc64", target_arch = "mips64")
))]
const fn fs_ioc_setversion() -> libc::c_ulong {
    0x8008_7602
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "32",
    not(any(target_arch = "powerpc", target_arch = "mips"))
))]
const fn fs_ioc_getflags() -> libc::c_ulong {
    0x8004_6601
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "32",
    not(any(target_arch = "powerpc", target_arch = "mips"))
))]
const fn fs_ioc_setflags() -> libc::c_ulong {
    0x4004_6602
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "32",
    not(any(target_arch = "powerpc", target_arch = "mips"))
))]
const fn fs_ioc_getversion() -> libc::c_ulong {
    0x8004_7601
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "32",
    not(any(target_arch = "powerpc", target_arch = "mips"))
))]
const fn fs_ioc_setversion() -> libc::c_ulong {
    0x4004_7602
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    not(any(
        target_arch = "powerpc64",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "mips"
    ))
))]
const fn fs_ioc_getflags() -> libc::c_ulong {
    0x8008_6601
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    not(any(
        target_arch = "powerpc64",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "mips"
    ))
))]
const fn fs_ioc_setflags() -> libc::c_ulong {
    0x4008_6602
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    not(any(
        target_arch = "powerpc64",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "mips"
    ))
))]
const fn fs_ioc_getversion() -> libc::c_ulong {
    0x8008_7601
}

#[cfg(all(
    target_os = "linux",
    target_pointer_width = "64",
    not(any(
        target_arch = "powerpc64",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "mips"
    ))
))]
const fn fs_ioc_setversion() -> libc::c_ulong {
    0x4008_7602
}

#[cfg(test)]
mod tests {
    use super::{format_long_flags, format_short_flags, parse_flag_mask};

    #[test]
    fn parses_known_flag_masks() {
        assert_eq!(parse_flag_mask("ia").unwrap(), 0x10 | 0x20);
    }

    #[test]
    fn formats_short_flags() {
        assert_eq!(format_short_flags(0x10 | 0x20), "-----ia------");
    }

    #[test]
    fn formats_long_flags() {
        assert_eq!(format_long_flags(0), "---");
        assert_eq!(format_long_flags(0x10 | 0x20), "Immutable, Append_Only");
    }
}
