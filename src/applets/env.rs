use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use crate::common::args::ArgCursor;
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
    let env = build_environment(&options);
    if options.command.is_empty() {
        print_environment(&env, options.nul_terminated)?;
        return Ok(0);
    }
    run_command(&env, &options.command)
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if cursor.parsing_flags() && (arg == "-" || arg == "-i") {
            options.clear = true;
            continue;
        }

        if cursor.parsing_flags() && arg == "-0" {
            options.nul_terminated = true;
            continue;
        }

        if cursor.parsing_flags() && arg == "-u" {
            options
                .unset
                .push(cursor.next_value(APPLET, "u")?.to_owned());
            continue;
        }

        if cursor.parsing_flags() && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }

        if let Some((name, value)) = arg.split_once('=')
            && !name.is_empty()
        {
            options
                .assignments
                .push((name.to_string(), value.to_string()));
            continue;
        }

        options.command.push(arg.to_owned());
        options.command.extend(cursor.remaining().iter().cloned());
        break;
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
    use super::parse_args;

    #[test]
    fn parses_assignments_and_command() {
        let options = parse_args(&[
            "-i".to_string(),
            "-u".to_string(),
            "HOME".to_string(),
            "FOO=bar".to_string(),
            "printenv".to_string(),
            "FOO".to_string(),
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
