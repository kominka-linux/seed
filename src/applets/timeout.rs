use std::os::unix::process::ExitStatusExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::common::applet::finish_code_or;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "timeout";
const EXIT_TIMEOUT: i32 = 124;
const EXIT_CANNOT_INVOKE: i32 = 126;
const EXIT_NOT_FOUND: i32 = 127;
const TERM_GRACE_PERIOD: Duration = Duration::from_millis(100);

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code_or(run(args), 125)
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let (duration, command, command_args) = parse_args(&args)?;

    let mut child = spawn_command(&command, &command_args).map_err(|e| {
            let code = match e.kind() {
                std::io::ErrorKind::NotFound => EXIT_NOT_FOUND,
                _ => EXIT_CANNOT_INVOKE,
            };
            vec![AppletError::new(
                APPLET,
                format!("failed to execute '{command}' (exit {code}): {e}"),
            )]
        })?;

    let deadline = Instant::now() + Duration::from_secs_f64(duration);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| {
            vec![AppletError::new(
                APPLET,
                format!("waiting for '{command}': {e}"),
            )]
        })? {
            return Ok(exit_code(status));
        }

        if Instant::now() >= deadline {
            terminate_child(&mut child, &command)?;
            return Ok(EXIT_TIMEOUT);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn spawn_command(command: &str, command_args: &[String]) -> std::io::Result<std::process::Child> {
    let mut process = Command::new(command);
    process.args(command_args);
    // SAFETY: `pre_exec` runs in the child just before `execve`; setting a new
    // process group isolates the command so timeout signals reach its descendants.
    unsafe {
        process.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    process.spawn()
}

fn terminate_child(child: &mut std::process::Child, command: &str) -> Result<(), Vec<AppletError>> {
    let pid = i32::try_from(child.id()).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid process id for '{command}'"),
        )]
    })?;

    signal_process_group(pid, libc::SIGTERM, command)?;
    let grace_deadline = Instant::now() + TERM_GRACE_PERIOD;
    loop {
        if child.try_wait().map_err(|e| {
            vec![AppletError::new(
                APPLET,
                format!("waiting for '{command}': {e}"),
            )]
        })?.is_some()
        {
            return Ok(());
        }
        if Instant::now() >= grace_deadline {
            signal_process_group(pid, libc::SIGKILL, command)?;
            let _ = child.wait();
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn signal_process_group(pid: i32, signal: libc::c_int, command: &str) -> Result<(), Vec<AppletError>> {
    // SAFETY: negative PID targets the spawned process group; arguments are plain integers.
    let status = unsafe { libc::kill(-pid, signal) };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("signaling '{command}': {error}"),
        )])
    }
}

fn parse_args(args: &[String]) -> Result<(f64, String, Vec<String>), Vec<AppletError>> {
    let Some(duration_arg) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    let duration = crate::applets::sleep::parse_duration(duration_arg).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid time interval '{duration_arg}'"),
        )]
    })?;

    let Some(command) = args.get(1) else {
        return Err(vec![AppletError::new(APPLET, "missing command operand")]);
    };

    Ok((duration, command.clone(), args[2..].to_vec()))
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    128 + status.signal().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{EXIT_TIMEOUT, parse_args, run};
    use crate::common::args::argv_to_strings;
    use std::ffi::OsString;
    use std::time::{Duration, Instant};

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_duration_and_command() {
        let argv = argv_to_strings("timeout", &args(&["1.5", "echo", "hi"])).unwrap();
        let (duration, command, rest) = parse_args(&argv).unwrap();
        assert_eq!(duration, 1.5);
        assert_eq!(command, "echo");
        assert_eq!(rest, vec![String::from("hi")]);
    }

    #[test]
    fn missing_command_errors() {
        let argv = argv_to_strings("timeout", &args(&["1"])).unwrap();
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn propagates_child_exit_code() {
        let code = run(&args(&["1", "sh", "-c", "exit 7"])).unwrap();
        assert_eq!(code, 7);
    }

    #[test]
    fn times_out_long_running_command() {
        let code = run(&args(&["0.05", "sh", "-c", "sleep 1"])).unwrap();
        assert_eq!(code, EXIT_TIMEOUT);
    }

    #[test]
    fn kills_term_ignoring_process_group_quickly() {
        let start = Instant::now();
        let code = run(&args(&["0.05", "sh", "-c", "trap '' TERM; sleep 1"])).unwrap();
        assert_eq!(code, EXIT_TIMEOUT);
        assert!(start.elapsed() < Duration::from_millis(500));
    }
}
