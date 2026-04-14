use std::env;
use std::ffi::{CStr, CString};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus};

use crate::common::account::{PasswdEntry, ShadowRecord, account_paths, find_passwd, read_passwd, read_shadow};
use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "login";
const DEFAULT_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    preserve_env: bool,
    host: Option<String>,
    force_user: Option<String>,
    user: Option<String>,
}

unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> AppletCodeResult {
    let options = parse_args(args)?;
    let paths = account_paths();
    let passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;

    let user_name = match options.force_user.clone().or(options.user.clone()) {
        Some(user) => user,
        None => prompt_username()?,
    };
    let user = find_passwd(&passwd, &user_name)
        .cloned()
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("unknown user {user_name}"))])?;

    if options.force_user.is_none() {
        authenticate(&user_name, &shadow)?;
    }

    let status = spawn_login_shell(&user, &options)
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", None, err)])?;
    Ok(exit_code(status))
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options {
        preserve_env: false,
        host: None,
        force_user: None,
        user: None,
    };

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => {
                if options.user.is_some() || options.force_user.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                options.user = Some(value.to_string());
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'p' => options.preserve_env = true,
                        'h' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.host = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "h")?
                                    .to_string(),
                            );
                            break;
                        }
                        'f' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.force_user = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "f")?
                                    .to_string(),
                            );
                            break;
                        }
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    Ok(options)
}

fn prompt_username() -> Result<String, Vec<AppletError>> {
    let mut out = io::stdout().lock();
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
    Ok(input)
}

fn authenticate(user: &str, shadow: &[ShadowRecord]) -> Result<(), Vec<AppletError>> {
    let hash = shadow.iter().find_map(|record| match record {
        ShadowRecord::Entry(entry) if entry.username == user => Some(entry.password.as_str()),
        _ => None,
    });
    let Some(hash) = hash else {
        return Err(vec![AppletError::new(APPLET, format!("unknown user {user}"))]);
    };

    let mut out = io::stdout().lock();
    write!(out, "Password: ").map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let mut password = String::new();
    BufReader::new(io::stdin().lock())
        .read_line(&mut password)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
    while matches!(password.as_bytes().last(), Some(b'\n' | b'\r')) {
        password.pop();
    }

    if verify_password(&password, hash) {
        Ok(())
    } else {
        Err(vec![AppletError::new(APPLET, "incorrect password")])
    }
}

fn verify_password(password: &str, hash: &str) -> bool {
    if hash.starts_with('!') {
        return false;
    }
    let Ok(password) = CString::new(password) else {
        return false;
    };
    let Ok(hash_c) = CString::new(hash) else {
        return false;
    };
    let computed = unsafe { crypt(password.as_ptr(), hash_c.as_ptr()) };
    if computed.is_null() {
        return false;
    }
    unsafe { CStr::from_ptr(computed) }.to_bytes() == hash_c.as_bytes()
}

fn spawn_login_shell(user: &PasswdEntry, options: &Options) -> io::Result<ExitStatus> {
    let shell = if user.shell.is_empty() {
        String::from("/bin/sh")
    } else {
        user.shell.clone()
    };
    let mut command = Command::new(&shell);
    command.arg0(format!("-{}", login_name(&shell)));
    command.uid(user.uid);
    command.gid(user.gid);
    command.env_clear();
    if options.preserve_env {
        for (key, value) in env::vars_os() {
            command.env(key, value);
        }
    }
    if let Ok(term) = env::var("TERM") {
        command.env("TERM", term);
    }
    if options.preserve_env {
        if env::var_os("PATH").is_none() {
            command.env("PATH", DEFAULT_PATH);
        }
    } else {
        command.env("HOME", &user.home);
        command.env("SHELL", &shell);
        command.env("USER", &user.username);
        command.env("LOGNAME", &user.username);
        command.env("PATH", env::var("PATH").unwrap_or_else(|_| String::from(DEFAULT_PATH)));
    }
    if let Some(host) = &options.host {
        command.env("REMOTEHOST", host);
    }

    let home = user.home.clone();
    unsafe {
        command.pre_exec(move || {
            env::set_current_dir(&home)?;
            Ok(())
        });
    }

    command.status()
}

fn login_name(shell: &str) -> &str {
    shell.rsplit('/').next().unwrap_or(shell)
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_force_login() {
        let options = parse_args(&args(&["-p", "-h", "host", "-f", "root"])).unwrap();
        assert!(options.preserve_env);
        assert_eq!(options.host.as_deref(), Some("host"));
        assert_eq!(options.force_user.as_deref(), Some("root"));
    }
}
