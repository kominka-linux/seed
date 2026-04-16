use std::collections::BTreeSet;
use std::io::Write;

use crate::common::applet::finish_code;
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::list_processes;

const APPLET: &str = "pidof";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    single: bool,
    omit: BTreeSet<i32>,
    names: Vec<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args).map(match_exit_code))
}

fn run(args: &[std::ffi::OsString]) -> Result<bool, Vec<AppletError>> {
    let mut options = parse_args(args)?;
    options.omit.insert(std::process::id() as i32);

    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let mut matches = Vec::new();

    for process in processes {
        if options.omit.contains(&process.pid) {
            continue;
        }
        if options
            .names
            .iter()
            .any(|name| process.matches_exact_name(name))
        {
            matches.push(process.pid);
            if options.single {
                break;
            }
        }
    }

    if matches.is_empty() {
        return Ok(false);
    }

    let mut out = stdout();
    let line = matches
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    writeln!(out, "{line}")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(true)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('s') => options.single = true,
            ParsedArg::Short('o') => {
                options.omit.insert(parse_omit_pid(&parser.value_str("o")?)?);
            }
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => options.names.push(arg.into_string().map_err(|arg| {
                vec![AppletError::new(
                    APPLET,
                    format!("argument is invalid unicode: {:?}", arg),
                )]
            })?),
        }
    }

    if options.names.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(options)
}

fn parse_omit_pid(value: &str) -> Result<i32, Vec<AppletError>> {
    if value == "%PPID" {
        return Ok(unsafe { libc::getppid() });
    }
    value
        .parse::<i32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid pid '{value}'"))])
}

fn match_exit_code(matched: bool) -> i32 {
    if matched { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_single_and_omit() {
        assert_eq!(
            parse_args(&args(&["-s", "-o", "12", "sleep"])).unwrap(),
            Options {
                single: true,
                omit: [12].into_iter().collect(),
                names: vec!["sleep".to_string()],
            }
        );
    }

    #[test]
    fn match_exit_codes_follow_pidof_convention() {
        assert_eq!(super::match_exit_code(true), 0);
        assert_eq!(super::match_exit_code(false), 1);
    }
}
