#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
use std::os::unix::process::ExitStatusExt;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::Command;

use crate::common::error::AppletError;

const APPLET: &str = "chroot";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let (new_root, command, command_args) = parse_args(args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(&new_root, &command, &command_args);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = (new_root, command, command_args);

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

#[cfg(target_os = "linux")]
fn run_linux(
    new_root: &str,
    command: &str,
    command_args: &[String],
) -> Result<i32, Vec<AppletError>> {
    let root = CString::new(Path::new(new_root).as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let slash = CString::new("/").expect("static string has no NUL");

    let mut child = Command::new(command);
    child.args(command_args);

    // SAFETY: the closure runs immediately before exec in the child. It only
    // calls libc setup functions and converts failures into io::Error.
    unsafe {
        child.pre_exec(move || {
            if libc::chroot(root.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::chdir(slash.as_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let status = child.status().map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("failed to execute '{command}': {err}"),
        )]
    })?;

    Ok(exit_code(status))
}

fn parse_args(args: &[String]) -> Result<(String, String, Vec<String>), Vec<AppletError>> {
    let Some(new_root) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };

    if args.len() == 1 {
        return Ok((
            new_root.clone(),
            String::from("/bin/sh"),
            vec![String::from("-i")],
        ));
    }

    Ok((new_root.clone(), args[1].clone(), args[2..].to_vec()))
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        code
    } else {
        128 + status.signal().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{exit_code, parse_args};
    use std::os::unix::process::ExitStatusExt;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_root_and_command() {
        let argv = args(&["/", "/bin/echo", "hi"]);
        let (root, command, rest) = parse_args(&argv).expect("parse chroot");
        assert_eq!(root, "/");
        assert_eq!(command, "/bin/echo");
        assert_eq!(rest, &["hi".to_string()]);
    }

    #[test]
    fn defaults_to_interactive_shell() {
        let argv = args(&["/"]);
        let (root, command, rest) = parse_args(&argv).expect("parse chroot");
        assert_eq!(root, "/");
        assert_eq!(command, "/bin/sh");
        assert_eq!(rest, &["-i".to_string()]);
    }

    #[test]
    fn propagates_signal_exit_codes() {
        let status = std::process::ExitStatus::from_raw(9);
        assert_eq!(exit_code(status), 137);
    }
}
