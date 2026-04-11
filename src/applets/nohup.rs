use std::fs::OpenOptions;
use std::os::unix::io::IntoRawFd;
use std::os::unix::process::ExitStatusExt;

use crate::common::error::AppletError;

const APPLET: &str = "nohup";

pub fn main(args: &[String]) -> i32 {
    let args = if args.first().map(|a| a.as_str()) == Some("--") {
        &args[1..]
    } else {
        args
    };

    match run(args) {
        Ok(code) => code,
        Err(errors) => {
            for e in errors {
                e.print();
            }
            125
        }
    }
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    // SAFETY: ignoring SIGHUP so the child survives terminal disconnect.
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };

    // If stdout is a tty, redirect to nohup.out.
    if unsafe { libc::isatty(libc::STDOUT_FILENO) } != 0 {
        let out_path = std::env::var("HOME")
            .ok()
            .map(|h| format!("{h}/nohup.out"))
            .unwrap_or_else(|| "nohup.out".to_string());

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(&out_path)
            .map_err(|e| vec![AppletError::from_io(APPLET, "opening", Some(&out_path), e)])?;

        // SAFETY: dup2 with valid file descriptors.
        let fd = file.into_raw_fd();
        unsafe {
            libc::dup2(fd, libc::STDOUT_FILENO);
            libc::close(fd);
        }

        eprintln!("nohup: appending output to '{out_path}'");
    }

    // If stderr is a tty, redirect it to stdout.
    if unsafe { libc::isatty(libc::STDERR_FILENO) } != 0 {
        // SAFETY: STDOUT_FILENO is a valid open descriptor at this point.
        unsafe { libc::dup2(libc::STDOUT_FILENO, libc::STDERR_FILENO) };
    }

    let status = std::process::Command::new(&args[0])
        .args(&args[1..])
        .status()
        .map_err(|e| {
            vec![AppletError::new(
                APPLET,
                format!("failed to execute '{}': {e}", args[0]),
            )]
        })?;

    Ok(match status.code() {
        Some(code) => code,
        None => 128 + status.signal().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::{main, run};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn missing_operand_is_an_error() {
        assert!(run(&[]).is_err());
        assert_eq!(main(&[]), 125);
    }

    #[test]
    fn nonexistent_command_is_an_error() {
        assert!(run(&args(&["__seed_command_does_not_exist__"])).is_err());
    }
}
