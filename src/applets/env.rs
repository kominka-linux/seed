use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "env";

#[derive(Debug, Default)]
struct Options {
    clear: bool,
    nul_terminated: bool,
    unset: Vec<String>,
    assignments: Vec<(String, String)>,
    command: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let env = build_environment(&options);
    if options.command.is_empty() {
        print_environment(&env, options.nul_terminated)?;
        return Ok(0);
    }
    run_command(&env, &options.command)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('i') => {
                options.clear = true;
                continue;
            }
            ParsedArg::Short('0') => {
                options.nul_terminated = true;
                continue;
            }
            ParsedArg::Short('u') => {
                options.unset.push(parser.value_str("u")?);
                continue;
            }
            ParsedArg::Short(flag) => {
                return Err(vec![AppletError::invalid_option(APPLET, flag)]);
            }
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ParsedArg::Value(arg) => {
                if arg == "-" {
                    options.clear = true;
                    continue;
                }

                let arg = arg
                    .into_string()
                    .map_err(|arg| vec![AppletError::new(APPLET, format!("argument is invalid unicode: {:?}", arg))])?;

                if let Some((name, value)) = arg.split_once('=')
                    && !name.is_empty()
                {
                    options
                        .assignments
                        .push((name.to_string(), value.to_string()));
                    continue;
                }

                options.command.push(arg);
                options.command.extend(
                    parser
                        .raw_args()?
                        .map(|value| value.to_string_lossy().into_owned()),
                );
                break;
            }
        }
    }

    Ok(options)
}

fn build_environment(options: &Options) -> Vec<(OsString, OsString)> {
    let mut env = if options.clear {
        Vec::new()
    } else {
        std::env::vars_os().collect::<Vec<_>>()
    };

    for name in &options.unset {
        let name = OsStr::new(name);
        env.retain(|(key, _)| key.as_os_str() != name);
    }

    for (name, value) in &options.assignments {
        let name = OsStr::new(name);
        if let Some((_, existing_value)) = env.iter_mut().find(|(key, _)| key.as_os_str() == name) {
            *existing_value = OsString::from(value);
        } else {
            env.push((OsString::from(name), OsString::from(value)));
        }
    }

    env
}

fn print_environment(
    env: &[(OsString, OsString)],
    nul_terminated: bool,
) -> Result<(), Vec<AppletError>> {
    let mut out = stdout();
    let separator = if nul_terminated { b'\0' } else { b'\n' };

    for (name, value) in env {
        out.write_all(name.as_os_str().as_bytes())
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        out.write_all(b"=")
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        out.write_all(value.as_os_str().as_bytes())
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        out.write_all(&[separator])
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    }

    Ok(())
}

fn run_command(env: &[(OsString, OsString)], command: &[String]) -> Result<i32, Vec<AppletError>> {
    let mut child = Command::new(&command[0]);
    child
        .args(&command[1..])
        .env_clear()
        .envs(env.iter().cloned());
    let status = child.status().map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("executing {}: {err}", command[0]),
        )]
    })?;

    Ok(match status.code() {
        Some(code) => code,
        None => 128 + status.signal().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::parse_args;

    #[test]
    fn parses_assignments_and_command() {
        let options = parse_args(&[
            OsString::from("-i"),
            OsString::from("-u"),
            OsString::from("HOME"),
            OsString::from("FOO=bar"),
            OsString::from("printenv"),
            OsString::from("FOO"),
        ])
        .expect("parse env args");

        assert!(options.clear);
        assert_eq!(options.unset, vec![String::from("HOME")]);
        assert_eq!(
            options.assignments,
            vec![(String::from("FOO"), String::from("bar"))]
        );
        assert_eq!(
            options.command,
            vec![String::from("printenv"), String::from("FOO")]
        );
    }
}
