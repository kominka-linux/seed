
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use crate::common::applet::finish_code;
use crate::common::args::Parser;
use crate::common::error::AppletError;
use crate::common::fstab;

const APPLET_SWAPON: &str = "swapon";
const APPLET_SWAPOFF: &str = "swapoff";
const SWAP_FLAG_PREFER: libc::c_int = 0x8000;
const SWAP_FLAG_PRIO_MASK: libc::c_int = 0x7fff;
const SWAP_FLAG_DISCARD: libc::c_int = 0x10000;
const SWAP_FLAG_DISCARD_ONCE: libc::c_int = 0x20000;
const SWAP_FLAG_DISCARD_PAGES: libc::c_int = 0x40000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Swapon,
    Swapoff,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    if_exists: bool,
    discard: Option<Option<String>>,
    priority: Option<i32>,
    devices: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(Action::Swapon, args))
}

pub fn main_swapoff(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(Action::Swapoff, args))
}

fn run(action: Action, args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(action, args)?;
    let devices = selected_devices(action, &options)?;
    if devices.is_empty() {
        return Ok(0);
    }

    let mut exit_code = 0;
    for device in devices {
        match apply_device(action, &device, &options) {
            Ok(()) => {}
            Err(error) => {
                exit_code = 1;
                if !(action == Action::Swapon
                    && options.if_exists
                    && error.raw_os_error() == Some(libc::ENOENT))
                {
                    AppletError::new(action.applet_name(), format!("{device}: {error}")).print();
                }
            }
        }
    }
    Ok(exit_code)
}

fn parse_args(action: Action, args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parser = Parser::new(action.applet_name(), args);
    let mut args = parser.raw_args()?;

    while let Some(arg) = args.next() {
        let arg = to_string(action.applet_name(), arg)?;
        match arg.as_str() {
            "-a" => options.all = true,
            "-e" if action == Action::Swapon => options.if_exists = true,
            "-p" if action == Action::Swapon => {
                let Some(value) = args.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET_SWAPON, "p")]);
                };
                let value = to_string(action.applet_name(), value)?;
                options.priority = Some(value.parse::<i32>().map_err(|_| {
                    vec![AppletError::new(
                        APPLET_SWAPON,
                        format!("invalid priority '{value}'"),
                    )]
                })?);
            }
            _ if action == Action::Swapon && arg.starts_with("-d") => {
                let value = arg.strip_prefix("-d").unwrap();
                options.discard = if value.is_empty() {
                    Some(None)
                } else {
                    Some(Some(value.to_string()))
                };
            }
            _ if arg.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    action.applet_name(),
                    arg.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => options.devices.push(arg.clone()),
        }
    }

    if !options.all && options.devices.is_empty() {
        return Err(vec![AppletError::new(
            action.applet_name(),
            "missing operand",
        )]);
    }

    Ok(options)
}

fn to_string(applet: &'static str, value: OsString) -> Result<String, Vec<AppletError>> {
    value.into_string().map_err(|value| {
        vec![AppletError::new(
            applet,
            format!("argument is invalid unicode: {:?}", value),
        )]
    })
}

fn selected_devices(action: Action, options: &Options) -> Result<Vec<String>, Vec<AppletError>> {
    let mut devices = BTreeSet::new();
    if options.all {
        if action == Action::Swapoff {
            for device in proc_swaps()? {
                devices.insert(device);
            }
        }
        for entry in fstab::read_entries()
            .map_err(|err| vec![AppletError::from_io(action.applet_name(), "reading", Some("/etc/fstab"), err)])?
        {
            if entry.vfstype != "swap" {
                continue;
            }
            if action == Action::Swapon && fstab::option_present(&entry.mntops, "noauto") {
                continue;
            }
            devices.insert(resolve_spec(&entry.spec));
        }
    }

    for device in &options.devices {
        devices.insert(resolve_spec(device));
    }

    Ok(devices.into_iter().collect())
}

fn proc_swaps() -> Result<Vec<String>, Vec<AppletError>> {
    let path = std::env::var_os("SEED_PROC_SWAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc/swaps"));
    let text = fs::read_to_string(&path).map_err(|err| {
        vec![AppletError::from_io(
            APPLET_SWAPOFF,
            "reading",
            Some(&path.to_string_lossy()),
            err,
        )]
    })?;
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter(|path| path.starts_with('/'))
        .map(str::to_string)
        .collect())
}

fn apply_device(action: Action, device: &str, options: &Options) -> Result<(), std::io::Error> {
    if action == Action::Swapon && options.if_exists && !std::path::Path::new(device).exists() {
        return Ok(());
    }

    match action {
        Action::Swapon => {
            let flags = effective_swapon_flags(device, options);
            let device_cstr = std::ffi::CString::new(device).map_err(invalid_input)?;
            // SAFETY: `device_cstr` points to a NUL-terminated path and `flags` is built from Linux swapon flags.
            let rc = unsafe { swapon_sys(device_cstr.as_ptr(), flags) };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        }
        Action::Swapoff => {
            let device_cstr = std::ffi::CString::new(device).map_err(invalid_input)?;
            // SAFETY: `device_cstr` points to a valid NUL-terminated path.
            let rc = unsafe { swapoff_sys(device_cstr.as_ptr()) };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        }
    }
}

fn effective_swapon_flags(device: &str, options: &Options) -> libc::c_int {
    let mut merged = options.clone();
    if let Ok(entries) = fstab::read_entries()
        && let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.vfstype == "swap" && resolve_spec(&entry.spec) == device)
    {
        if merged.priority.is_none() {
            merged.priority = fstab::option_value(&entry.mntops, "pri")
                .and_then(|value| value.parse::<i32>().ok());
        }
        if merged.discard.is_none() && fstab::option_present(&entry.mntops, "discard") {
            merged.discard = Some(fstab::option_value(&entry.mntops, "discard").map(str::to_string));
        }
    }
    swapon_flags(&merged)
}

fn swapon_flags(options: &Options) -> libc::c_int {
    let mut flags = 0;
    if let Some(priority) = options.priority {
        flags |= SWAP_FLAG_PREFER | priority.clamp(0, SWAP_FLAG_PRIO_MASK);
    }
    if let Some(policy) = &options.discard {
        flags |= SWAP_FLAG_DISCARD;
        match policy.as_deref() {
            Some("once") => flags |= SWAP_FLAG_DISCARD_ONCE,
            Some("pages") => flags |= SWAP_FLAG_DISCARD_PAGES,
            Some(_) | None => {
                flags |= SWAP_FLAG_DISCARD_ONCE | SWAP_FLAG_DISCARD_PAGES;
            }
        }
    }
    flags
}

fn resolve_spec(spec: &str) -> String {
    if let Some(value) = spec.strip_prefix("UUID=") {
        return format!("/dev/disk/by-uuid/{value}");
    }
    if let Some(value) = spec.strip_prefix("LABEL=") {
        return format!("/dev/disk/by-label/{value}");
    }
    spec.to_string()
}

fn invalid_input(_: std::ffi::NulError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "path contains NUL byte")
}

impl Action {
    fn applet_name(self) -> &'static str {
        match self {
            Action::Swapon => APPLET_SWAPON,
            Action::Swapoff => APPLET_SWAPOFF,
        }
    }
}

unsafe extern "C" {
    #[link_name = "swapon"]
    fn swapon_sys(path: *const libc::c_char, flags: libc::c_int) -> libc::c_int;
    #[link_name = "swapoff"]
    fn swapoff_sys(path: *const libc::c_char) -> libc::c_int;
}

#[cfg(test)]
mod tests {
    use super::{Action, Options, effective_swapon_flags, parse_args, selected_devices, swapon_flags};
    use crate::common::test_env;
    use std::ffi::OsString;
    use std::fs;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_swapon_flags() {
        assert_eq!(
            parse_args(Action::Swapon, &args(&["-a", "-e", "-dpages", "-p", "9"])).unwrap(),
            Options {
                all: true,
                if_exists: true,
                discard: Some(Some("pages".to_string())),
                priority: Some(9),
                devices: Vec::new(),
            }
        );
    }

    #[test]
    fn computes_swapon_flags() {
        let flags = swapon_flags(&Options {
            priority: Some(3),
            discard: Some(Some("once".to_string())),
            ..Options::default()
        });
        assert_ne!(flags & 0x8000, 0);
        assert_ne!(flags & 0x20000, 0);
    }

    #[test]
    fn all_mode_collects_fstab_and_proc_swaps() {
        let _guard = test_env::lock();
        let fstab_path = std::env::temp_dir().join(format!("seed-swap-{}.fstab", std::process::id()));
        let proc_path = std::env::temp_dir().join(format!("seed-swap-{}.proc", std::process::id()));
        fs::write(&fstab_path, "/swapfile none swap defaults 0 0\n").unwrap();
        fs::write(&proc_path, "Filename\tType\tSize\tUsed\tPriority\n/active swap 0 0 -2\n").unwrap();
        unsafe {
            std::env::set_var("SEED_FSTAB", &fstab_path);
            std::env::set_var("SEED_PROC_SWAPS", &proc_path);
        }
        let devices = selected_devices(
            Action::Swapoff,
            &Options {
                all: true,
                ..Options::default()
            },
        )
        .unwrap();
        unsafe {
            std::env::remove_var("SEED_FSTAB");
            std::env::remove_var("SEED_PROC_SWAPS");
        }
        assert_eq!(devices, vec!["/active".to_string(), "/swapfile".to_string()]);
        let _ = fs::remove_file(fstab_path);
        let _ = fs::remove_file(proc_path);
    }

    #[test]
    fn swapon_all_uses_fstab_priority_and_discard() {
        let _guard = test_env::lock();
        let fstab_path = std::env::temp_dir().join(format!("seed-swap-flags-{}.fstab", std::process::id()));
        fs::write(&fstab_path, "/swapfile none swap pri=7,discard=pages 0 0\n").unwrap();
        unsafe {
            std::env::set_var("SEED_FSTAB", &fstab_path);
        }
        let flags = effective_swapon_flags("/swapfile", &Options::default());
        unsafe {
            std::env::remove_var("SEED_FSTAB");
        }
        assert_ne!(flags & 0x8000, 0);
        assert_ne!(flags & 0x40000, 0);
        let _ = fs::remove_file(fstab_path);
    }
}
