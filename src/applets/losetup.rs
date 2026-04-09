#[cfg(target_os = "linux")]
use std::fs::{self, File, OpenOptions};
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::path::Path;

use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
#[cfg(target_os = "linux")]
use crate::common::io::stdout;

const APPLET: &str = "losetup";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
enum Command {
    List,
    FindFree,
    Attach {
        selector: LoopSelector,
        file: String,
    },
    SetCapacity {
        device: String,
    },
    Detach {
        device: String,
    },
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
enum LoopSelector {
    NextFree,
    Device(String),
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
struct Options {
    offset: u64,
    partscan: bool,
    read_only: bool,
    command: Command,
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    #[cfg(target_os = "linux")]
    {
        run_linux(options)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = options;
        Err(vec![AppletError::new(
            APPLET,
            "not supported on this platform",
        )])
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut offset = 0_u64;
    let mut partscan = false;
    let mut read_only = false;
    let mut find_free = false;
    let mut list = false;
    let mut capacity_device: Option<String> = None;
    let mut detach_device: Option<String> = None;
    let mut positional = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if arg == "-o" {
            let value = cursor.next_value(APPLET, "o")?;
            offset = value.parse::<u64>().map_err(|_| {
                vec![AppletError::new(
                    APPLET,
                    format!("invalid offset '{value}'"),
                )]
            })?;
            continue;
        }

        if arg == "-c" {
            capacity_device = Some(cursor.next_value(APPLET, "c")?.to_owned());
            continue;
        }

        if arg == "-d" {
            detach_device = Some(cursor.next_value(APPLET, "d")?.to_owned());
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'a' => list = true,
                    'f' => find_free = true,
                    'P' => partscan = true,
                    'r' => read_only = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }

        positional.push(arg.to_owned());
    }

    let command_count = usize::from(list)
        + usize::from(find_free)
        + usize::from(capacity_device.is_some())
        + usize::from(detach_device.is_some());
    if command_count > 1 {
        return Err(vec![AppletError::new(
            APPLET,
            "conflicting actions specified",
        )]);
    }

    let command = if let Some(device) = capacity_device {
        if !positional.is_empty() {
            return Err(vec![AppletError::new(APPLET, "unexpected operand")]);
        }
        Command::SetCapacity { device }
    } else if let Some(device) = detach_device {
        if !positional.is_empty() {
            return Err(vec![AppletError::new(APPLET, "unexpected operand")]);
        }
        Command::Detach { device }
    } else if list {
        if !positional.is_empty() {
            return Err(vec![AppletError::new(APPLET, "unexpected operand")]);
        }
        Command::List
    } else if find_free {
        match positional.len() {
            0 => Command::FindFree,
            1 => Command::Attach {
                selector: LoopSelector::NextFree,
                file: positional[0].clone(),
            },
            _ => return Err(vec![AppletError::new(APPLET, "unexpected operand")]),
        }
    } else {
        match positional.as_slice() {
            [device, file] => Command::Attach {
                selector: LoopSelector::Device(device.clone()),
                file: file.clone(),
            },
            [] => return Err(vec![AppletError::new(APPLET, "missing operand")]),
            _ => return Err(vec![AppletError::new(APPLET, "unexpected operand")]),
        }
    };

    if !matches!(command, Command::Attach { .. }) && (offset != 0 || partscan || read_only) {
        return Err(vec![AppletError::new(
            APPLET,
            "options '-o', '-P', and '-r' require attach mode",
        )]);
    }

    Ok(Options {
        offset,
        partscan,
        read_only,
        command,
    })
}

#[cfg(target_os = "linux")]
fn run_linux(options: Options) -> Result<(), Vec<AppletError>> {
    match options.command {
        Command::List => list_loop_devices(),
        Command::FindFree => {
            let path = find_free_loop_device()?;
            writeln!(stdout(), "{path}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])
        }
        Command::Attach { selector, file } => attach_loop_device(
            selector,
            &file,
            options.offset,
            options.read_only,
            options.partscan,
        ),
        Command::SetCapacity { device } => set_capacity(&device),
        Command::Detach { device } => detach_loop_device(&device),
    }
}

#[cfg(target_os = "linux")]
fn attach_loop_device(
    selector: LoopSelector,
    file: &str,
    offset: u64,
    read_only: bool,
    partscan: bool,
) -> Result<(), Vec<AppletError>> {
    let loop_path = match selector {
        LoopSelector::NextFree => find_free_loop_device()?,
        LoopSelector::Device(path) => path,
    };

    let backing = OpenOptions::new()
        .read(true)
        .write(!read_only)
        .open(file)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(file), err)])?;
    let loop_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&loop_path)
        .map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "opening",
                Some(loop_path.as_str()),
                err,
            )]
        })?;

    // SAFETY: `ioctl` is called with a valid loop-device fd and a valid backing-file fd.
    let set_fd_status =
        unsafe { libc::ioctl(loop_file.as_raw_fd(), LOOP_SET_FD, backing.as_raw_fd()) };
    if set_fd_status < 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!(
                "attaching {} to {}: {}",
                file,
                loop_path,
                std::io::Error::last_os_error()
            ),
        )]);
    }

    let mut info = LoopInfo64::default();
    info.lo_offset = offset;
    info.lo_flags = loop_flags(read_only, partscan);
    copy_file_name(&mut info.lo_file_name, file.as_bytes());

    // SAFETY: `info` points to a valid `loop_info64` value for the duration
    // of the ioctl, and `loop_file` is a valid loop-device fd.
    let status = unsafe {
        libc::ioctl(
            loop_file.as_raw_fd(),
            LOOP_SET_STATUS64,
            &info as *const LoopInfo64,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        let _ = clear_loop_fd(loop_file.as_raw_fd());
        Err(vec![AppletError::new(
            APPLET,
            format!("configuring {}: {}", loop_path, err),
        )])
    }
}

#[cfg(target_os = "linux")]
fn set_capacity(device: &str) -> Result<(), Vec<AppletError>> {
    let loop_file = open_loop_device(device)?;
    // SAFETY: `loop_file` is a valid loop-device fd.
    let status = unsafe { libc::ioctl(loop_file.as_raw_fd(), LOOP_SET_CAPACITY) };
    if status == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!(
                "setting capacity on {device}: {}",
                std::io::Error::last_os_error()
            ),
        )])
    }
}

#[cfg(target_os = "linux")]
fn detach_loop_device(device: &str) -> Result<(), Vec<AppletError>> {
    let loop_file = open_loop_device(device)?;
    clear_loop_fd(loop_file.as_raw_fd()).map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("detaching {device}: {err}"),
        )]
    })
}

#[cfg(target_os = "linux")]
fn list_loop_devices() -> Result<(), Vec<AppletError>> {
    let mut devices = fs::read_dir("/dev")
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/dev"), err)])?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let name = file_name.to_str()?;
            if name.starts_with("loop") && name[4..].chars().all(|ch| ch.is_ascii_digit()) {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    devices.sort();

    let mut out = stdout();
    for path in devices {
        if let Some(line) = loop_device_status(path.as_path())? {
            writeln!(out, "{line}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn loop_device_status(path: &Path) -> Result<Option<String>, Vec<AppletError>> {
    let loop_file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", path.to_str(), err)])?;
    let mut info = LoopInfo64::default();
    // SAFETY: `info` points to a valid `loop_info64`, and `loop_file` is a valid fd.
    let status = unsafe {
        libc::ioctl(
            loop_file.as_raw_fd(),
            LOOP_GET_STATUS64,
            &mut info as *mut LoopInfo64,
        )
    };
    if status == 0 {
        let backing = c_string_from_bytes(&info.lo_file_name);
        Ok(Some(format!("{}: ({backing})", path.display())))
    } else {
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(code) if code == libc::ENXIO || code == libc::ENODEV => Ok(None),
            _ => Err(vec![AppletError::new(
                APPLET,
                format!("reading {}: {err}", path.display()),
            )]),
        }
    }
}

#[cfg(target_os = "linux")]
fn find_free_loop_device() -> Result<String, Vec<AppletError>> {
    let control = File::open("/dev/loop-control").map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "opening",
            Some("/dev/loop-control"),
            err,
        )]
    })?;

    // SAFETY: `control` is a valid loop-control fd and the ioctl takes no extra pointer args.
    let index = unsafe { libc::ioctl(control.as_raw_fd(), LOOP_CTL_GET_FREE) };
    if index < 0 {
        Err(vec![AppletError::new(
            APPLET,
            format!(
                "finding free loop device: {}",
                std::io::Error::last_os_error()
            ),
        )])
    } else {
        Ok(format!("/dev/loop{index}"))
    }
}

#[cfg(target_os = "linux")]
fn open_loop_device(path: &str) -> Result<File, Vec<AppletError>> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(path), err)])
}

#[cfg(target_os = "linux")]
fn clear_loop_fd(fd: libc::c_int) -> Result<(), std::io::Error> {
    // SAFETY: `fd` is expected to refer to an open loop device. The ioctl takes no pointer args.
    let status = unsafe { libc::ioctl(fd, LOOP_CLR_FD) };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn loop_flags(read_only: bool, partscan: bool) -> u32 {
    let mut flags = 0;
    if read_only {
        flags |= LO_FLAGS_READ_ONLY;
    }
    if partscan {
        flags |= LO_FLAGS_PARTSCAN;
    }
    flags
}

#[cfg(target_os = "linux")]
fn copy_file_name(dest: &mut [u8; LO_NAME_SIZE], bytes: &[u8]) {
    let len = bytes.len().min(dest.len().saturating_sub(1));
    dest[..len].copy_from_slice(&bytes[..len]);
}

#[cfg(target_os = "linux")]
fn c_string_from_bytes(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(target_os = "linux")]
const LO_NAME_SIZE: usize = 64;
#[cfg(target_os = "linux")]
const LO_KEY_SIZE: usize = 32;
#[cfg(target_os = "linux")]
const LOOP_SET_FD: libc::c_int = 0x4C00;
#[cfg(target_os = "linux")]
const LOOP_CLR_FD: libc::c_int = 0x4C01;
#[cfg(target_os = "linux")]
const LOOP_SET_STATUS64: libc::c_int = 0x4C04;
#[cfg(target_os = "linux")]
const LOOP_GET_STATUS64: libc::c_int = 0x4C05;
#[cfg(target_os = "linux")]
const LOOP_SET_CAPACITY: libc::c_int = 0x4C07;
#[cfg(target_os = "linux")]
const LOOP_CTL_GET_FREE: libc::c_int = 0x4C82;
#[cfg(target_os = "linux")]
const LO_FLAGS_READ_ONLY: u32 = 1;
#[cfg(target_os = "linux")]
const LO_FLAGS_PARTSCAN: u32 = 8;

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct LoopInfo64 {
    lo_device: u64,
    lo_inode: u64,
    lo_rdevice: u64,
    lo_offset: u64,
    lo_sizelimit: u64,
    lo_number: u32,
    lo_encrypt_type: u32,
    lo_encrypt_key_size: u32,
    lo_flags: u32,
    lo_file_name: [u8; LO_NAME_SIZE],
    lo_crypt_name: [u8; LO_NAME_SIZE],
    lo_encrypt_key: [u8; LO_KEY_SIZE],
    lo_init: [u64; 2],
}

#[cfg(target_os = "linux")]
impl Default for LoopInfo64 {
    fn default() -> Self {
        Self {
            lo_device: 0,
            lo_inode: 0,
            lo_rdevice: 0,
            lo_offset: 0,
            lo_sizelimit: 0,
            lo_number: 0,
            lo_encrypt_type: 0,
            lo_encrypt_key_size: 0,
            lo_flags: 0,
            lo_file_name: [0; LO_NAME_SIZE],
            lo_crypt_name: [0; LO_NAME_SIZE],
            lo_encrypt_key: [0; LO_KEY_SIZE],
            lo_init: [0; 2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, LoopSelector, parse_args};

    #[test]
    fn parses_find_free_attach() {
        let options = parse_args(&["-f".to_string(), "disk.img".to_string()]).expect("parse");
        match options.command {
            Command::Attach {
                selector: LoopSelector::NextFree,
                file,
            } => assert_eq!(file, "disk.img"),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_detach() {
        let options = parse_args(&["-d".to_string(), "/dev/loop0".to_string()]).expect("parse");
        match options.command {
            Command::Detach { device } => assert_eq!(device, "/dev/loop0"),
            _ => panic!("unexpected command"),
        }
    }
}
