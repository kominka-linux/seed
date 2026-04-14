
use std::ffi::CString;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;

use crate::common::applet::finish_code;
use crate::common::error::AppletError;
use crate::common::mounts::{self, MountInfo};

const APPLET: &str = "umount";
const LOOP_CLR_FD: libc::c_int = 0x4C01;
const MNT_FORCE: libc::c_int = 1;
const MNT_DETACH: libc::c_int = 2;
const MS_REMOUNT: libc::c_ulong = 32;
const MS_RDONLY: libc::c_ulong = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    remount_readonly: bool,
    lazy: bool,
    force: bool,
    free_loop: bool,
    type_filter: Option<Vec<String>>,
    option_filter: Option<Vec<String>>,
    operands: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let targets = unmount_targets(&options)?;
    if targets.is_empty() {
        return Ok(0);
    }

    let mut exit_code = 0;
    for target in targets {
        if let Err(error) = unmount_target(&options, &target) {
            exit_code = 1;
            error.print();
        }
    }

    Ok(exit_code)
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        match arg.as_str() {
            "-a" => options.all = true,
            "-r" => options.remount_readonly = true,
            "-l" => options.lazy = true,
            "-f" => options.force = true,
            "-d" => options.free_loop = true,
            "-t" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "t")]);
                };
                index += 1;
                options.type_filter = Some(split_csv(value));
            }
            "-O" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "O")]);
                };
                index += 1;
                options.option_filter = Some(split_csv(value));
            }
            _ if arg.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    arg.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => options.operands.push(arg.clone()),
        }
    }

    if !options.all && options.operands.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(options)
}

fn unmount_targets(options: &Options) -> Result<Vec<MountInfo>, Vec<AppletError>> {
    let mounts = mounts::read_mountinfo()
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/proc/self/mountinfo"), err)])?;
    if options.all {
        let mut entries = mounts
            .into_iter()
            .filter(|entry| entry.mounted_on != "/")
            .filter(|entry| {
                options
                    .type_filter
                    .as_ref()
                    .is_none_or(|items| items.iter().any(|item| item == &entry.filesystem))
            })
            .filter(|entry| {
                options.option_filter.as_ref().is_none_or(|items| {
                    items.iter().all(|item| {
                        entry.options.iter().any(|option: &String| {
                            option == item
                                || option
                                    .split_once('=')
                                    .is_some_and(|(name, _)| name == item)
                        })
                    })
                })
            })
            .collect::<Vec<_>>();
        entries.reverse();
        return Ok(entries);
    }

        Ok(options
            .operands
            .iter()
            .map(|operand| {
                mounts
                    .iter()
                    .find(|entry| entry.mounted_on == *operand || entry.source == *operand)
                    .cloned()
                    .unwrap_or(MountInfo {
                        source: operand.clone(),
                        filesystem: String::new(),
                        mounted_on: operand.clone(),
                        options: Vec::new(),
                    })
            })
            .collect::<Vec<_>>())
}

fn unmount_target(options: &Options, entry: &MountInfo) -> Result<(), AppletError> {
    let target = CString::new(entry.mounted_on.as_str())
        .map_err(|_| AppletError::new(APPLET, "target contains NUL byte"))?;
    let mut flags = 0;
    if options.lazy {
        flags |= MNT_DETACH;
    }
    if options.force {
        flags |= MNT_FORCE;
    }

    // SAFETY: `target` is a valid NUL-terminated string for the duration of the call.
    let rc = unsafe { umount2_sys(target.as_ptr(), flags) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if options.remount_readonly && err.raw_os_error() == Some(libc::EBUSY) {
            remount_readonly(entry)?;
        } else {
            return Err(AppletError::new(
                APPLET,
                format!("unmounting {}: {}", entry.mounted_on, err),
            ));
        }
    }

    if options.free_loop && entry.source.starts_with("/dev/loop") {
        free_loop_device(&entry.source)?;
    }
    Ok(())
}

fn remount_readonly(entry: &MountInfo) -> Result<(), AppletError> {
    let target = CString::new(entry.mounted_on.as_str())
        .map_err(|_| AppletError::new(APPLET, "target contains NUL byte"))?;
    // SAFETY: `target` is a valid NUL-terminated string; null pointers are permitted for source and fstype on remount.
    let rc = unsafe {
        mount_sys(
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            MS_REMOUNT | MS_RDONLY,
            std::ptr::null(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(AppletError::new(
            APPLET,
            format!(
                "remounting {} read-only: {}",
                entry.mounted_on,
                std::io::Error::last_os_error()
            ),
        ))
    }
}

fn free_loop_device(path: &str) -> Result<(), AppletError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| AppletError::from_io(APPLET, "opening", Some(path), err))?;
    // SAFETY: `file` is an open loop device and the ioctl takes no pointer argument.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), LOOP_CLR_FD as _, 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(AppletError::new(
            APPLET,
            format!("detaching {}: {}", path, std::io::Error::last_os_error()),
        ))
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

unsafe extern "C" {
    #[link_name = "umount2"]
    fn umount2_sys(target: *const libc::c_char, flags: libc::c_int) -> libc::c_int;
    #[link_name = "mount"]
    fn mount_sys(
        source: *const libc::c_char,
        target: *const libc::c_char,
        filesystemtype: *const libc::c_char,
        mountflags: libc::c_ulong,
        data: *const libc::c_void,
    ) -> libc::c_int;
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_filters() {
        let options = parse_args(&args(&["-a", "-r", "-l", "-f", "-d", "-t", "tmpfs", "-O", "nosuid"])).unwrap();
        assert!(options.all);
        assert!(options.remount_readonly);
        assert!(options.lazy);
        assert!(options.force);
        assert!(options.free_loop);
        assert_eq!(options.type_filter, Some(vec!["tmpfs".to_string()]));
        assert_eq!(options.option_filter, Some(vec!["nosuid".to_string()]));
    }
}
