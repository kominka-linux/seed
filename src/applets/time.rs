use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::time::Instant;

use crate::common::applet::finish_code_or;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "time";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code_or(run(args), 125)
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let command_args: &[String] = if args.first().map(String::as_str) == Some("--") {
        &args[1..]
    } else {
        &args
    };

    let Some(command) = command_args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };

    let usage_before = get_rusage_children()?;
    let started = Instant::now();
    let status = Command::new(command)
        .args(&command_args[1..])
        .status()
        .map_err(|err| {
            vec![AppletError::new(
                APPLET,
                format!("failed to execute '{command}': {err}"),
            )]
        })?;
    let elapsed = started.elapsed();
    let usage_after = get_rusage_children()?;

    let user = timeval_diff(usage_before.ru_utime, usage_after.ru_utime);
    let sys = timeval_diff(usage_before.ru_stime, usage_after.ru_stime);

    let mut stderr = std::io::stderr().lock();
    writeln!(stderr, "real {}", format_duration(elapsed.as_secs_f64()))
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stderr", None, err)])?;
    writeln!(stderr, "user {}", format_duration(user))
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stderr", None, err)])?;
    writeln!(stderr, "sys {}", format_duration(sys))
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stderr", None, err)])?;

    Ok(exit_code(status))
}

fn get_rusage_children() -> Result<libc::rusage, Vec<AppletError>> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to valid writable memory and the selector is a valid libc constant.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, usage.as_mut_ptr()) };
    if rc == 0 {
        // SAFETY: libc initialized `usage` on success.
        Ok(unsafe { usage.assume_init() })
    } else {
        Err(vec![AppletError::from_io(
            APPLET,
            "collecting resource usage",
            None,
            std::io::Error::last_os_error(),
        )])
    }
}

fn timeval_diff(before: libc::timeval, after: libc::timeval) -> f64 {
    let seconds = (after.tv_sec - before.tv_sec) as f64;
    let micros = (after.tv_usec - before.tv_usec) as f64 / 1_000_000.0;
    seconds + micros
}

fn format_duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as u64;
    let seconds = seconds - (minutes as f64 * 60.0);
    format!("{minutes}m{seconds:.3}s")
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
    use super::{format_duration, run};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn formats_duration() {
        assert_eq!(format_duration(0.125), "0m0.125s");
        assert_eq!(format_duration(61.5), "1m1.500s");
    }

    #[test]
    fn propagates_child_exit_code() {
        let code = run(&args(&["sh", "-c", "exit 7"])).unwrap();
        assert_eq!(code, 7);
    }
}
