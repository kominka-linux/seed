use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::common::applet::finish_code_or;
use crate::common::error::AppletError;

const APPLET: &str = "timeout";
const EXIT_TIMEOUT: i32 = 124;
const EXIT_CANNOT_INVOKE: i32 = 126;
const EXIT_NOT_FOUND: i32 = 127;

pub fn main(args: &[String]) -> i32 {
    finish_code_or(run(args), 125)
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let (duration, command, command_args) = parse_args(args)?;

    let mut child = Command::new(command)
        .args(command_args)
        .spawn()
        .map_err(|e| {
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
            unsafe {
                libc::kill(child.id() as i32, libc::SIGTERM);
            }
            let _ = child.wait();
            return Ok(EXIT_TIMEOUT);
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn parse_args(args: &[String]) -> Result<(f64, &str, &[String]), Vec<AppletError>> {
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

    Ok((duration, command.as_str(), &args[2..]))
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

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_duration_and_command() {
        let argv = args(&["1.5", "echo", "hi"]);
        let (duration, command, rest) = parse_args(&argv).unwrap();
        assert_eq!(duration, 1.5);
        assert_eq!(command, "echo");
        assert_eq!(rest, &["hi".to_string()]);
    }

    #[test]
    fn missing_command_errors() {
        assert!(parse_args(&args(&["1"])).is_err());
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
}
