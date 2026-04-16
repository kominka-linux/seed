use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "setsid";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let (command, rest) = parse_args(&args)?;
    let mut child = Command::new(command);
    child.args(rest);

    // SAFETY: the closure only calls async-signal-safe libc `setsid` and
    // converts the result into an `io::Error` for exec setup.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
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

fn parse_args(args: &[String]) -> Result<(&str, &[String]), Vec<AppletError>> {
    let Some(command) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing command operand")]);
    };
    Ok((command.as_str(), &args[1..]))
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        code
    } else {
        128 + status.signal().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{parse_args, run};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn os_args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_command() {
        let argv = args(&["echo", "hi"]);
        let (command, rest) = parse_args(&argv).expect("parse setsid");
        assert_eq!(command, "echo");
        assert_eq!(rest, &["hi".to_string()]);
    }

    #[test]
    fn propagates_child_exit_code() {
        let code = run(&os_args(&["sh", "-c", "exit 7"])).expect("run setsid");
        assert_eq!(code, 7);
    }
}
