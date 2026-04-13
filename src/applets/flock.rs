use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::error::AppletError;

const APPLET: &str = "flock";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    mode: LockMode,
    nonblock: bool,
    path: String,
    command: CommandSpec,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandSpec {
    Args(Vec<String>),
    Shell(String),
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&options.path)
        .map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "opening",
                Some(&options.path),
                err,
            )]
        })?;

    let mut operation = match options.mode {
        LockMode::Shared => libc::LOCK_SH,
        LockMode::Exclusive => libc::LOCK_EX,
    };
    if options.nonblock {
        operation |= libc::LOCK_NB;
    }

    // SAFETY: `file` is a valid open descriptor and `operation` is composed
    // from valid `flock(2)` flags.
    let rc = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if rc != 0 {
        let error = std::io::Error::last_os_error();
        if options.nonblock && error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(1);
        }
        return Err(vec![AppletError::from_io(
            APPLET,
            "locking",
            Some(&options.path),
            error,
        )]);
    }

    run_command(&options.command)
}

fn run_command(command: &CommandSpec) -> Result<i32, Vec<AppletError>> {
    let status = match command {
        CommandSpec::Args(args) => {
            let (program, rest) = args.split_first().expect("args command is non-empty");
            Command::new(program).args(rest).status()
        }
        CommandSpec::Shell(command) => Command::new("sh").arg("-c").arg(command).status(),
    }
    .map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("failed to execute command: {err}"),
        )]
    })?;

    Ok(exit_code(status))
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut mode = LockMode::Exclusive;
    let mut nonblock = false;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        if arg == "--" {
            index += 1;
            break;
        }
        if arg == "-n" {
            nonblock = true;
            index += 1;
            continue;
        }
        if arg == "-s" {
            mode = LockMode::Shared;
            index += 1;
            continue;
        }
        if arg == "-x" {
            mode = LockMode::Exclusive;
            index += 1;
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }
        break;
    }

    let Some(path) = args.get(index) else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    index += 1;

    if args.get(index).map(String::as_str) == Some("-c") {
        let Some(command) = args.get(index + 1) else {
            return Err(vec![AppletError::option_requires_arg(APPLET, "c")]);
        };
        if args.len() != index + 2 {
            return Err(vec![AppletError::new(APPLET, "extra operand")]);
        }
        return Ok(Options {
            mode,
            nonblock,
            path: path.clone(),
            command: CommandSpec::Shell(command.clone()),
        });
    }

    let command = args[index..].to_vec();
    if command.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing command operand")]);
    }

    Ok(Options {
        mode,
        nonblock,
        path: path.clone(),
        command: CommandSpec::Args(command),
    })
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
    use super::{CommandSpec, LockMode, Options, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_nonblocking_command_args() {
        let options = parse_args(&args(&["-n", "lock", "echo", "hi"])).expect("parse flock");
        assert_eq!(
            options,
            Options {
                mode: LockMode::Exclusive,
                nonblock: true,
                path: String::from("lock"),
                command: CommandSpec::Args(vec![String::from("echo"), String::from("hi")]),
            }
        );
    }

    #[test]
    fn parses_c_command() {
        let options = parse_args(&args(&["-s", "lock", "-c", "echo hi"])).expect("parse flock");
        assert_eq!(
            options,
            Options {
                mode: LockMode::Shared,
                nonblock: false,
                path: String::from("lock"),
                command: CommandSpec::Shell(String::from("echo hi")),
            }
        );
    }
}
