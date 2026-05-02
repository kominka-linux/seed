
use std::env;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::args::{ArgCursor, ArgToken};
use crate::common::applet::finish_code;
use crate::common::error::AppletError;
use crate::common::fstab;
use crate::common::mounts;

const APPLET: &str = "mount";
const MS_RDONLY: libc::c_ulong = 1;
const MS_NOSUID: libc::c_ulong = 2;
const MS_NODEV: libc::c_ulong = 4;
const MS_NOEXEC: libc::c_ulong = 8;
const MS_SYNCHRONOUS: libc::c_ulong = 16;
const MS_REMOUNT: libc::c_ulong = 32;
const MS_DIRSYNC: libc::c_ulong = 128;
const MS_NOATIME: libc::c_ulong = 1024;
const MS_NODIRATIME: libc::c_ulong = 2048;
const MS_BIND: libc::c_ulong = 4096;
const MS_MOVE: libc::c_ulong = 8192;
const MS_REC: libc::c_ulong = 16384;
const MS_UNBINDABLE: libc::c_ulong = 1 << 17;
const MS_PRIVATE: libc::c_ulong = 1 << 18;
const MS_SLAVE: libc::c_ulong = 1 << 19;
const MS_SHARED: libc::c_ulong = 1 << 20;
const MS_RELATIME: libc::c_ulong = 1 << 21;
const HELPER_PATHS: [&str; 2] = ["/sbin", "/usr/sbin"];

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    dry_run: bool,
    verbose: bool,
    readonly: bool,
    type_filter: Option<Vec<String>>,
    option_filter: Option<Vec<String>>,
    mount_options: Vec<String>,
    ignore_helpers: bool,
    operands: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MountRequest {
    source: String,
    target: String,
    fstype: Option<String>,
    options: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ParsedMountOptions {
    flags: libc::c_ulong,
    data: Vec<String>,
    propagation_flags: Vec<libc::c_ulong>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    if options.operands.is_empty() && !options.all {
        list_mounts()?;
        return Ok(0);
    }

    let requests = mount_requests(&options)?;
    for request in requests {
        mount_request(&options, &request)?;
    }
    Ok(0)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options::default();

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => options.operands.push(value.to_string()),
            ArgToken::LongOption("all", None) => options.all = true,
            ArgToken::LongOption("fake", None) => options.dry_run = true,
            ArgToken::LongOption("internal-only", None) => options.ignore_helpers = true,
            ArgToken::LongOption("verbose", None) => options.verbose = true,
            ArgToken::LongOption("read-only", None) | ArgToken::LongOption("readonly", None) => {
                options.readonly = true;
            }
            ArgToken::LongOption("types", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "t")?;
                options.type_filter = Some(split_csv(value));
            }
            ArgToken::LongOption("test-opts", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "O")?;
                options.option_filter = Some(split_csv(value));
            }
            ArgToken::LongOption("options", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "o")?;
                options.mount_options.extend(split_csv(value));
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                if flags.starts_with('-') {
                    return Err(vec![AppletError::invalid_option(APPLET, '-')]);
                }

                if flags.len() != 1 {
                    return Err(vec![AppletError::invalid_option(
                        APPLET,
                        flags.chars().next().unwrap_or('-'),
                    )]);
                }

                match flags.as_bytes()[0] as char {
                    'a' => options.all = true,
                    'f' => options.dry_run = true,
                    'i' => options.ignore_helpers = true,
                    'v' => options.verbose = true,
                    'r' => options.readonly = true,
                    't' => {
                        let value = cursor.next_value(APPLET, "t")?;
                        options.type_filter = Some(split_csv(value));
                    }
                    'O' => {
                        let value = cursor.next_value(APPLET, "O")?;
                        options.option_filter = Some(split_csv(value));
                    }
                    'o' => {
                        let value = cursor.next_value(APPLET, "o")?;
                        options.mount_options.extend(split_csv(value));
                    }
                    other => {
                        return Err(vec![AppletError::invalid_option(APPLET, other)]);
                    }
                }
            }
        }
    }

    Ok(options)
}

fn list_mounts() -> Result<(), Vec<AppletError>> {
    let entries = mounts::read_mountinfo()
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/proc/self/mountinfo"), err)])?;
    for entry in entries {
        println!(
            "{} on {} type {} ({})",
            entry.source,
            entry.mounted_on,
            entry.filesystem,
            entry.options.join(",")
        );
    }
    Ok(())
}

fn mount_requests(options: &Options) -> Result<Vec<MountRequest>, Vec<AppletError>> {
    if options.all {
        let mut entries = fstab::read_entries()
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/etc/fstab"), err)])?;
        entries.retain(|entry| !fstab::option_present(&entry.mntops, "noauto"));
        if let Some(filter) = options.type_filter.as_ref() {
            entries.retain(|entry| fstab::matches_type_filter(Some(filter), &entry.vfstype));
        }
        if let Some(filter) = options.option_filter.as_ref() {
            entries.retain(|entry| fstab::matches_option_filter(Some(filter), &entry.mntops));
        }
        return Ok(entries
            .into_iter()
            .map(|entry| MountRequest {
                source: resolve_spec(&entry.spec),
                target: entry.file,
                fstype: normalize_fstype(&entry.vfstype),
                options: entry.mntops,
            })
            .collect());
    }

    match options.operands.as_slice() {
        [single] => {
            let entries = fstab::read_entries()
                .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/etc/fstab"), err)])?;
            let entry = entries
                .into_iter()
                .find(|entry| entry.spec == *single || entry.file == *single)
                .ok_or_else(|| vec![AppletError::new(APPLET, format!("no fstab entry for '{single}'"))])?;
            Ok(vec![MountRequest {
                source: resolve_spec(&entry.spec),
                target: entry.file,
                fstype: normalize_fstype(&entry.vfstype),
                options: entry.mntops,
            }])
        }
        [source, target] => Ok(vec![MountRequest {
            source: resolve_spec(source),
            target: target.clone(),
            fstype: options.type_filter.as_ref().and_then(|items| {
                (items.len() == 1 && !items[0].starts_with("no")).then(|| items[0].clone())
            }),
            options: Vec::new(),
        }]),
        [] => Ok(Vec::new()),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn mount_request(options: &Options, request: &MountRequest) -> Result<(), Vec<AppletError>> {
    let mut effective_options = request.options.clone();
    effective_options.extend(options.mount_options.iter().cloned());
    if options.readonly {
        effective_options.push(String::from("ro"));
    }
    let parsed = parse_mount_options(&effective_options);
    if options.verbose {
        eprintln!(
            "{} on {} type {} ({})",
            request.source,
            request.target,
            request.fstype.as_deref().unwrap_or("auto"),
            effective_options.join(",")
        );
    }
    if options.dry_run {
        return Ok(());
    }

    if let Some(helper) = mount_helper_path(options, request, &parsed) {
        return run_mount_helper(&helper, request, &effective_options);
    }

    let source = CString::new(request.source.as_str())
        .map_err(|_| vec![AppletError::new(APPLET, "source contains NUL byte")])?;
    let target = CString::new(request.target.as_str())
        .map_err(|_| vec![AppletError::new(APPLET, "target contains NUL byte")])?;
    let fstype = request
        .fstype
        .as_deref()
        .map(CString::new)
        .transpose()
        .map_err(|_| vec![AppletError::new(APPLET, "filesystem type contains NUL byte")])?;
    let data = if parsed.data.is_empty() {
        None
    } else {
        Some(
            CString::new(parsed.data.join(","))
                .map_err(|_| vec![AppletError::new(APPLET, "mount options contain NUL byte")])?,
        )
    };

    let source_ptr = source.as_ptr();
    let target_ptr = target.as_ptr();
    let fstype_ptr = fstype.as_ref().map_or(std::ptr::null(), |value| value.as_ptr());
    let data_ptr = data
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr() as *const libc::c_void);
    // SAFETY: pointers are valid NUL-terminated strings for the duration of the call.
    let rc = unsafe { mount_sys(source_ptr, target_ptr, fstype_ptr, parsed.flags, data_ptr) };
    if rc != 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("mounting {} on {}: {}", request.source, request.target, std::io::Error::last_os_error()),
        )]);
    }

    for propagation in parsed.propagation_flags {
        // SAFETY: target_ptr stays valid for the duration of this call; null pointers are permitted by mount(2) for propagation ops.
        let rc = unsafe {
            mount_sys(
                std::ptr::null(),
                target_ptr,
                std::ptr::null(),
                propagation,
                std::ptr::null(),
            )
        };
        if rc != 0 {
            return Err(vec![AppletError::new(
                APPLET,
                format!(
                    "applying propagation flags to {}: {}",
                    request.target,
                    std::io::Error::last_os_error()
                ),
            )]);
        }
    }

    Ok(())
}

fn mount_helper_path(
    options: &Options,
    request: &MountRequest,
    parsed: &ParsedMountOptions,
) -> Option<PathBuf> {
    if options.ignore_helpers || uses_direct_syscall_only(parsed) {
        return None;
    }
    let fstype = request.fstype.as_deref()?;
    helper_search_paths()
        .into_iter()
        .map(|dir| dir.join(format!("mount.{fstype}")))
        .find(|path| path.is_file())
}

fn helper_search_paths() -> Vec<PathBuf> {
    env::var_os("SEED_MOUNT_HELPERS")
        .map(|value| env::split_paths(&value).collect())
        .filter(|paths: &Vec<PathBuf>| !paths.is_empty())
        .unwrap_or_else(|| HELPER_PATHS.iter().map(PathBuf::from).collect())
}

fn uses_direct_syscall_only(parsed: &ParsedMountOptions) -> bool {
    parsed.flags & (MS_BIND | MS_MOVE | MS_REMOUNT) != 0 || !parsed.propagation_flags.is_empty()
}

fn run_mount_helper(
    helper: &Path,
    request: &MountRequest,
    options: &[String],
) -> Result<(), Vec<AppletError>> {
    let mut command = Command::new(helper);
    command.arg(&request.source).arg(&request.target);
    if !options.is_empty() {
        command.arg("-o").arg(options.join(","));
    }
    let status = command.status().map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "executing",
            Some(&helper.to_string_lossy()),
            err,
        )]
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("helper '{}' exited with status {}", helper.display(), status),
        )])
    }
}

fn parse_mount_options(options: &[String]) -> ParsedMountOptions {
    let mut parsed = ParsedMountOptions::default();
    for option in options {
        match option.as_str() {
            "defaults" | "loop" => {}
            "ro" => parsed.flags |= MS_RDONLY,
            "rw" => parsed.flags &= !MS_RDONLY,
            "sync" => parsed.flags |= MS_SYNCHRONOUS,
            "async" => parsed.flags &= !MS_SYNCHRONOUS,
            "dirsync" => parsed.flags |= MS_DIRSYNC,
            "atime" => parsed.flags &= !MS_NOATIME,
            "noatime" => parsed.flags |= MS_NOATIME,
            "diratime" => parsed.flags &= !MS_NODIRATIME,
            "nodiratime" => parsed.flags |= MS_NODIRATIME,
            "relatime" => parsed.flags |= MS_RELATIME,
            "norelatime" => parsed.flags &= !MS_RELATIME,
            "dev" => parsed.flags &= !MS_NODEV,
            "nodev" => parsed.flags |= MS_NODEV,
            "exec" => parsed.flags &= !MS_NOEXEC,
            "noexec" => parsed.flags |= MS_NOEXEC,
            "suid" => parsed.flags &= !MS_NOSUID,
            "nosuid" => parsed.flags |= MS_NOSUID,
            "bind" => parsed.flags |= MS_BIND,
            "rbind" => parsed.flags |= MS_BIND | MS_REC,
            "move" => parsed.flags |= MS_MOVE,
            "remount" => parsed.flags |= MS_REMOUNT,
            "shared" => parsed.propagation_flags.push(MS_SHARED),
            "rshared" => parsed.propagation_flags.push(MS_SHARED | MS_REC),
            "private" => parsed.propagation_flags.push(MS_PRIVATE),
            "rprivate" => parsed.propagation_flags.push(MS_PRIVATE | MS_REC),
            "slave" => parsed.propagation_flags.push(MS_SLAVE),
            "rslave" => parsed.propagation_flags.push(MS_SLAVE | MS_REC),
            "unbindable" => parsed.propagation_flags.push(MS_UNBINDABLE),
            "runbindable" => parsed.propagation_flags.push(MS_UNBINDABLE | MS_REC),
            other => parsed.data.push(other.to_string()),
        }
    }
    parsed
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_fstype(value: &str) -> Option<String> {
    match value {
        "" | "auto" => None,
        other => Some(other.to_string()),
    }
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

unsafe extern "C" {
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
    use super::{
        MS_BIND, MS_NOEXEC, Options, ParsedMountOptions, helper_search_paths, mount_helper_path,
        mount_requests, parse_args, parse_mount_options, uses_direct_syscall_only,
    };
    use crate::common::test_env;
    use std::fs;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_mount_flags() {
        let options = parse_args(&args(&["-a", "-f", "-r", "-t", "ext4,tmpfs", "-O", "noauto", "-o", "bind,noexec"])).unwrap();
        assert!(options.all);
        assert!(options.dry_run);
        assert!(options.readonly);
        assert_eq!(options.type_filter, Some(vec!["ext4".to_string(), "tmpfs".to_string()]));
        assert_eq!(options.option_filter, Some(vec!["noauto".to_string()]));
        assert_eq!(options.mount_options, vec!["bind".to_string(), "noexec".to_string()]);
    }

    #[test]
    fn parses_mount_option_flags() {
        let parsed = parse_mount_options(&["bind".to_string(), "noexec".to_string(), "size=4m".to_string()]);
        assert_ne!(parsed.flags & MS_BIND, 0);
        assert_ne!(parsed.flags & MS_NOEXEC, 0);
        assert_eq!(parsed.data, vec!["size=4m".to_string()]);
    }

    #[test]
    fn all_mode_uses_filtered_fstab_entries() {
        let _guard = test_env::lock();
        let path = std::env::temp_dir().join(format!("seed-mount-{}.fstab", std::process::id()));
        fs::write(
            &path,
            "/dev/root / ext4 defaults 0 0\n/dev/data /data xfs noauto 0 0\ntmpfs /run tmpfs defaults 0 0\n",
        )
        .unwrap();
        unsafe {
            std::env::set_var("SEED_FSTAB", &path);
        }
        let requests = mount_requests(&Options {
            all: true,
            type_filter: Some(vec!["tmpfs".to_string()]),
            ..Options::default()
        })
        .unwrap();
        unsafe {
            std::env::remove_var("SEED_FSTAB");
        }
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].target, "/run");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn uses_env_helper_search_path() {
        let _guard = test_env::lock();
        let helper_dir =
            std::env::temp_dir().join(format!("seed-mount-helper-{}", std::process::id()));
        fs::create_dir_all(&helper_dir).unwrap();
        unsafe {
            std::env::set_var("SEED_MOUNT_HELPERS", &helper_dir);
        }
        assert_eq!(helper_search_paths(), vec![helper_dir.clone()]);
        unsafe {
            std::env::remove_var("SEED_MOUNT_HELPERS");
        }
        let _ = fs::remove_dir_all(helper_dir);
    }

    #[test]
    fn helper_only_used_for_regular_filesystem_mounts() {
        let _guard = test_env::lock();
        let helper_dir =
            std::env::temp_dir().join(format!("seed-mount-helper-{}", std::process::id()));
        fs::create_dir_all(&helper_dir).unwrap();
        let helper = helper_dir.join("mount.testfs");
        fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&helper).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&helper, perms).unwrap();
        }
        unsafe {
            std::env::set_var("SEED_MOUNT_HELPERS", &helper_dir);
        }

        let request = super::MountRequest {
            source: String::from("/dev/fake"),
            target: String::from("/mnt"),
            fstype: Some(String::from("testfs")),
            options: Vec::new(),
        };
        assert_eq!(
            mount_helper_path(&Options::default(), &request, &ParsedMountOptions::default()),
            Some(helper.clone())
        );
        assert!(mount_helper_path(
            &Options {
                ignore_helpers: true,
                ..Options::default()
            },
            &request,
            &ParsedMountOptions::default(),
        )
        .is_none());
        assert!(uses_direct_syscall_only(&parse_mount_options(&[
            String::from("bind"),
            String::from("noexec"),
        ])));

        unsafe {
            std::env::remove_var("SEED_MOUNT_HELPERS");
        }
        let _ = fs::remove_dir_all(helper_dir);
    }
}
