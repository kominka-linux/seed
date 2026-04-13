use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::error::AppletError;

const APPLET: &str = "watch";

#[derive(Clone, Debug, PartialEq)]
struct Options {
    interval: f64,
    command: String,
    args: Vec<String>,
}

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
    let options = parse_args(args)?;

    loop {
        let status = Command::new(&options.command)
            .args(&options.args)
            .status()
            .map_err(|err| {
                vec![AppletError::new(
                    APPLET,
                    format!("failed to execute '{}': {err}", options.command),
                )]
            })?;
        let code = exit_code(status);
        if code == 127 {
            return Ok(code);
        }
        thread::sleep(Duration::from_secs_f64(options.interval));
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut interval = 2.0_f64;
    let mut index = 0;

    while let Some(arg) = args.get(index) {
        if arg == "--" {
            index += 1;
            break;
        }
        if arg == "-n" {
            let Some(value) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "n")]);
            };
            interval = parse_interval(value)?;
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("-n") {
            if value.is_empty() {
                return Err(vec![AppletError::option_requires_arg(APPLET, "n")]);
            }
            interval = parse_interval(value)?;
            index += 1;
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }
        break;
    }

    let Some(command) = args.get(index) else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };

    Ok(Options {
        interval,
        command: command.clone(),
        args: args[index + 1..].to_vec(),
    })
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
    use super::{Options, parse_args};

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
                command: String::from("echo"),
                args: vec![String::from("hi")],
            }
        );
    }

    #[test]
    fn rejects_missing_command() {
        assert!(parse_args(&args(&["-n", "1"])).is_err());
    }
}
