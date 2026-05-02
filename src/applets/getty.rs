use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, ExitStatus};

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "getty";
const DEFAULT_LOGIN: &str = "/bin/login";
const DEFAULT_ISSUE: &str = "/etc/issue";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    no_prompt: bool,
    no_issue: bool,
    issue_file: String,
    login: String,
    host: Option<String>,
    init_string: Option<String>,
    baud: String,
    tty: String,
    term: Option<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    let options = parse_args(args)?;
    let status = run_getty(&options)?;
    Ok(status.code().unwrap_or(1))
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut no_prompt = false;
    let mut no_issue = false;
    let mut issue_file = String::from(DEFAULT_ISSUE);
    let mut login = String::from(DEFAULT_LOGIN);
    let mut host = None;
    let mut init_string = None;
    let mut operands = Vec::new();

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => operands.push(value.to_string()),
            ArgToken::LongOption("no-prompt", None) => no_prompt = true,
            ArgToken::LongOption("no-issue", None) => no_issue = true,
            ArgToken::LongOption("issue-file", attached) => {
                issue_file = cursor
                    .next_value_or_maybe_attached(attached, APPLET, "f")?
                    .to_string();
            }
            ArgToken::LongOption("login-program", attached) => {
                login = cursor
                    .next_value_or_maybe_attached(attached, APPLET, "l")?
                    .to_string();
            }
            ArgToken::LongOption("init-string", attached) => {
                init_string = Some(
                    cursor
                        .next_value_or_maybe_attached(attached, APPLET, "I")?
                        .to_string(),
                );
            }
            ArgToken::LongOption("host", attached) => {
                host = Some(
                    cursor
                        .next_value_or_maybe_attached(attached, APPLET, "H")?
                        .to_string(),
                );
            }
            ArgToken::LongOption("timeout", attached) => {
                let _ = cursor.next_value_or_maybe_attached(attached, APPLET, "t")?;
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'n' => no_prompt = true,
                        'i' => no_issue = true,
                        'f' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            issue_file = cursor.next_value_or_attached(attached, APPLET, "f")?.to_string();
                            break;
                        }
                        'l' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            login = cursor.next_value_or_attached(attached, APPLET, "l")?.to_string();
                            break;
                        }
                        'I' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            init_string = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "I")?
                                    .to_string(),
                            );
                            break;
                        }
                        'H' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            host = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "H")?
                                    .to_string(),
                            );
                            break;
                        }
                        'h' | 'L' | 'm' | 'w' => {}
                        't' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let _ = cursor.next_value_or_attached(attached, APPLET, "t")?;
                            break;
                        }
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    let [baud, tty] = operands.as_slice() else {
        let [baud, tty, term] = operands.as_slice() else {
            return Err(vec![AppletError::new(APPLET, "missing operand")]);
        };
        return Ok(Options {
            no_prompt,
            no_issue,
            issue_file,
            login,
            host,
            init_string,
            baud: baud.clone(),
            tty: tty.clone(),
            term: Some(term.clone()),
        });
    };

    Ok(Options {
        no_prompt,
        no_issue,
        issue_file,
        login,
        host,
        init_string,
        baud: baud.clone(),
        tty: tty.clone(),
        term: None,
    })
}

fn run_getty(options: &Options) -> Result<ExitStatus, Vec<AppletError>> {
    let _ = (&options.baud, &options.tty);
    let mut out = io::stdout().lock();
    if let Some(init) = &options.init_string {
        write!(out, "{init}").map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    }
    if !options.no_issue {
        match fs::read_to_string(&options.issue_file) {
            Ok(issue) => {
                write!(out, "{issue}")
                    .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(&options.issue_file),
                    err,
                )]);
            }
        }
    }
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let username = if options.no_prompt {
        None
    } else {
        write!(out, "login: ").map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        out.flush()
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        let mut input = String::new();
        BufReader::new(io::stdin().lock())
            .read_line(&mut input)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        while matches!(input.as_bytes().last(), Some(b'\n' | b'\r')) {
            input.pop();
        }
        Some(input)
    };

    let mut command = Command::new(&options.login);
    if let Some(term) = &options.term {
        command.env("TERM", term);
    }
    if let Some(host) = &options.host {
        command.arg("-h").arg(host);
    }
    if let Some(username) = username {
        command.arg(username);
    }

    command
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", Some(&options.login), err)])
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_no_prompt_mode() {
        let options = parse_args(&args(&["-n", "-l", "/bin/login", "0", "/dev/null", "vt100"])).unwrap();
        assert!(options.no_prompt);
        assert_eq!(options.login, "/bin/login");
        assert_eq!(options.term.as_deref(), Some("vt100"));
    }
}
