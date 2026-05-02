
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::args::ArgCursor;
use crate::common::applet::finish_code;
use crate::common::error::AppletError;

const APPLET: &str = "switch_root";
const RAMFS_MAGIC: libc::c_long = 0x8584_58f6;
const TMPFS_MAGIC: libc::c_long = 0x0102_1994;
const MS_MOVE: libc::c_ulong = 8192;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    console: Option<String>,
    new_root: String,
    new_init: String,
    init_args: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    run_linux(&options)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut console = None;
    let mut operands = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        if cursor.parsing_flags() {
            match arg {
                "-c" => {
                    console = Some(cursor.next_value(APPLET, "c")?.to_owned());
                }
                value if value.starts_with('-') => {
                    return Err(vec![AppletError::invalid_option(
                        APPLET,
                        value.chars().nth(1).unwrap_or('-'),
                    )]);
                }
                value => {
                    operands.push(value.to_owned());
                    for operand in cursor.remaining() {
                        operands.push(crate::common::args::os_to_string(APPLET, operand.as_os_str())?);
                    }
                    break;
                }
            }
        } else {
            operands.push(arg.to_owned());
        }
    }

    let [new_root, new_init, rest @ ..] = operands.as_slice() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };

    Ok(Options {
        console,
        new_root: new_root.clone(),
        new_init: new_init.clone(),
        init_args: rest.to_vec(),
    })
}

fn run_linux(options: &Options) -> Result<i32, Vec<AppletError>> {
    validate_switch_root(options)?;

    if dry_run_enabled() {
        validate_dry_run_target(options)?;
        return Ok(0);
    }

    let root_dev = current_root_metadata()?.dev();
    delete_root_contents(Path::new("/"), root_dev, Path::new(&options.new_root))?;
    move_mount_to_root(Path::new(&options.new_root))?;

    let dot = CString::new(".").expect("static string has no NUL");
    let mut child = Command::new(&options.new_init);
    child.args(&options.init_args);
    let console = options.console.clone();

    // SAFETY: the closure runs in the child immediately before exec and only
    // performs libc mount/chroot/chdir/open/dup2 operations.
    unsafe {
        child.pre_exec(move || {
            if libc::chroot(dot.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::chdir(dot.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(console) = &console {
                let path = CString::new(console.as_bytes()).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "console contains NUL byte")
                })?;
                let fd = libc::open(path.as_ptr(), libc::O_RDWR);
                if fd < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::dup2(fd, libc::STDIN_FILENO) < 0
                    || libc::dup2(fd, libc::STDOUT_FILENO) < 0
                    || libc::dup2(fd, libc::STDERR_FILENO) < 0
                {
                    let err = std::io::Error::last_os_error();
                    libc::close(fd);
                    return Err(err);
                }
                if fd > libc::STDERR_FILENO {
                    libc::close(fd);
                }
            }
            Ok(())
        });
    }

    let err = child.exec();
    Err(vec![AppletError::new(
        APPLET,
        format!("can't execute '{}': {err}", options.new_init),
    )])
}

fn validate_switch_root(options: &Options) -> Result<(), Vec<AppletError>> {
    if current_pid() != 1 {
        return Err(vec![AppletError::new(APPLET, "PID must be 1")]);
    }

    let init_path = init_check_path();
    let init_meta = fs::metadata(&init_path).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "stat",
            Some(&init_path.to_string_lossy()),
            err,
        )]
    })?;
    if !init_meta.is_file() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("'{}' is not a regular file", init_path.display()),
        )]);
    }

    let root_magic = current_root_fs_type()?;
    if root_magic != RAMFS_MAGIC && root_magic != TMPFS_MAGIC {
        return Err(vec![AppletError::new(
            APPLET,
            "root filesystem is not ramfs/tmpfs",
        )]);
    }

    let root_meta = current_root_metadata()?;
    let new_root_meta = fs::metadata(&options.new_root).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "stat",
            Some(&options.new_root),
            err,
        )]
    })?;
    if !new_root_meta.is_dir() {
        return Err(vec![AppletError::new(APPLET, "NEW_ROOT must be a directory")]);
    }
    if !allow_same_device() && new_root_meta.dev() == root_meta.dev() {
        return Err(vec![AppletError::new(APPLET, "NEW_ROOT must be a mountpoint")]);
    }

    Ok(())
}

fn validate_dry_run_target(options: &Options) -> Result<(), Vec<AppletError>> {
    let target = dry_run_init_target(options);
    let meta = fs::metadata(&target).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "stat",
            Some(&target.to_string_lossy()),
            err,
        )]
    })?;
    if !meta.is_file() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("'{}' is not a regular file", target.display()),
        )]);
    }
    #[allow(clippy::unnecessary_cast)]
    if meta.mode() & 0o111 == 0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("can't execute '{}'", options.new_init),
        )]);
    }
    Ok(())
}

fn dry_run_init_target(options: &Options) -> PathBuf {
    let relative = options.new_init.strip_prefix('/').unwrap_or(&options.new_init);
    Path::new(&options.new_root).join(relative)
}

fn delete_root_contents(root: &Path, root_dev: u64, keep: &Path) -> Result<(), Vec<AppletError>> {
    let entries = fs::read_dir(root)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading directory", Some("/"), err)])?;
    for entry in entries {
        let entry = entry.map_err(|err| vec![AppletError::from_io(APPLET, "reading directory", Some("/"), err)])?;
        let path = entry.path();
        if path == keep {
            continue;
        }
        delete_path(&path, root_dev, keep)?;
    }
    Ok(())
}

fn delete_path(path: &Path, root_dev: u64, keep: &Path) -> Result<(), Vec<AppletError>> {
    if path == keep {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "stat",
                Some(&path.to_string_lossy()),
                err,
            )]);
        }
    };
    if metadata.dev() != root_dev {
        return Ok(());
    }
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in fs::read_dir(path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "reading directory",
                Some(&path.to_string_lossy()),
                err,
            )]
        })? {
            let entry = entry.map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "reading directory",
                    Some(&path.to_string_lossy()),
                    err,
                )]
            })?;
            delete_path(&entry.path(), root_dev, keep)?;
        }
        fs::remove_dir(path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "removing directory",
                Some(&path.to_string_lossy()),
                err,
            )]
        })?;
    } else {
        fs::remove_file(path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "removing",
                Some(&path.to_string_lossy()),
                err,
            )]
        })?;
    }
    Ok(())
}

fn move_mount_to_root(new_root: &Path) -> Result<(), Vec<AppletError>> {
    let source = CString::new(new_root.as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let target = CString::new("/").expect("static string has no NUL");
    // SAFETY: both pointers are valid C strings and the null fs/data pointers are permitted for MS_MOVE.
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            MS_MOVE,
            std::ptr::null(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("error moving root: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn current_pid() -> u32 {
    std::env::var("SEED_SWITCH_ROOT_TEST_PID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(std::process::id)
}

fn current_root_fs_type() -> Result<libc::c_long, Vec<AppletError>> {
    if let Ok(value) = std::env::var("SEED_SWITCH_ROOT_TEST_ROOTFS") {
        return parse_rootfs_override(&value);
    }

    let root = CString::new("/").expect("static string has no NUL");
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: the path pointer is valid and stat points to writable memory.
    let rc = unsafe { libc::statfs(root.as_ptr(), stat.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: statfs succeeded and initialized the structure.
        Ok(unsafe { stat.assume_init() }.f_type as libc::c_long)
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("statfs /: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn parse_rootfs_override(value: &str) -> Result<libc::c_long, Vec<AppletError>> {
    match value {
        "ramfs" => Ok(RAMFS_MAGIC),
        "tmpfs" => Ok(TMPFS_MAGIC),
        _ => value.parse::<libc::c_long>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid rootfs override '{value}'"),
            )]
        }),
    }
}

fn current_root_metadata() -> Result<fs::Metadata, Vec<AppletError>> {
    let path = std::env::var_os("SEED_SWITCH_ROOT_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    fs::metadata(&path).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "stat",
            Some(&path.to_string_lossy()),
            err,
        )]
    })
}

fn init_check_path() -> PathBuf {
    std::env::var_os("SEED_SWITCH_ROOT_TEST_INIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/init"))
}

fn allow_same_device() -> bool {
    std::env::var_os("SEED_SWITCH_ROOT_TEST_ALLOW_SAME_DEV").is_some()
}

fn dry_run_enabled() -> bool {
    std::env::var_os("SEED_SWITCH_ROOT_TEST_DRY_RUN").is_some()
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_console_and_init() {
        assert_eq!(
            parse_args(&args(&["-c", "/dev/console", "/newroot", "/sbin/init", "-s"])).unwrap(),
            Options {
                console: Some(String::from("/dev/console")),
                new_root: String::from("/newroot"),
                new_init: String::from("/sbin/init"),
                init_args: vec![String::from("-s")],
            }
        );
    }

    #[test]
    fn rejects_missing_operands() {
        assert!(parse_args(&args(&["/newroot"])).is_err());
    }
}
