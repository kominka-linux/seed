use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::applet::finish_code;
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "watch";

#[derive(Clone, Debug, PartialEq)]
struct Options {
    interval: f64,
    command: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let args = crate::common::args::argv_to_strings(APPLET, args)?;
    let options = parse_args(&args)?;

    loop {
        let status = Command::new("sh")
            .arg("-c")
            .arg(render_command(&options.command))
            .status()
            .map_err(|err| {
                vec![AppletError::new(
                    APPLET,
                    format!("failed to execute watch command: {err}"),
                )]
            })?;
        let _ = exit_code(status);
        thread::sleep(Duration::from_secs_f64(options.interval));
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut interval = 2.0_f64;
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::LongOption("interval", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "n")?;
                interval = parse_interval(value)?;
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.chars();
                let Some(flag) = chars.next() else {
                    continue;
                };
                let attached = chars.as_str();
                match flag {
                    'n' => {
                        let value = cursor.next_value_or_attached(attached, APPLET, "n")?;
                        interval = parse_interval(value)?;
                    }
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            ArgToken::Operand(command) => {
                return Ok(Options {
                    interval,
                    command: std::iter::once(command.to_owned())
                        .chain(cursor.remaining().iter().cloned())
                        .collect(),
                });
            }
        }
    }

    Err(vec![AppletError::new(APPLET, "missing operand")])
}

fn render_command(command: &[String]) -> String {
    if command.len() == 1 {
        return command[0].clone();
    }

    command
        .iter()
        .map(|arg| shell_escape(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_escape(arg: &str) -> String {
    if arg.is_empty() {
        return String::from("''");
    }
    if arg
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:".contains(&byte))
    {
        return arg.to_owned();
    }
    format!("'{}'", arg.replace('\'', r"'\''"))
}

fn parse_interval(value: &str) -> Result<f64, Vec<AppletError>> {
    crate::applets::sleep::parse_duration(value).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid time interval '{value}'"),
        )]
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
    use super::{Options, parse_args, render_command};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_interval_and_command() {
        let options = parse_args(&args(&["-n", "0.5", "echo", "hi"])).expect("parse watch");
        assert_eq!(
            options,
            Options {
                interval: 0.5,
                command: vec![String::from("echo"), String::from("hi")],
            }
        );
    }

    #[test]
    fn parses_attached_interval() {
        let options = parse_args(&args(&["-n0.5", "echo"])).expect("parse watch");
        assert_eq!(options.interval, 0.5);
        assert_eq!(options.command, vec![String::from("echo")]);
    }

    #[test]
    fn parses_long_interval() {
        let options = parse_args(&args(&["--interval=0.5", "echo"])).expect("parse watch");
        assert_eq!(options.interval, 0.5);
        assert_eq!(options.command, vec![String::from("echo")]);
    }

    #[test]
    fn rejects_missing_command() {
        assert!(parse_args(&args(&["-n", "1"])).is_err());
    }

    #[test]
    fn renders_multi_argument_command_for_shell() {
        assert_eq!(
            render_command(&args(&["echo", "hello world"])),
            "echo 'hello world'"
        );
        assert_eq!(render_command(&args(&["echo hi"])), "echo hi");
    }
}
