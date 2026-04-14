use std::env;
use std::ffi::{CStr, CString};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus};

use crate::common::account::{read_shadow, ShadowRecord, account_paths};
use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "su";
const DEFAULT_SHELL: &str = "/bin/sh";
const DEFAULT_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    login: bool,
    preserve_env: bool,
    shell: Option<String>,
    command: Option<String>,
    user: String,
    extra_args: Vec<String>,
}

#[derive(Clone, Debug)]
struct UserInfo {
    name: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
}

unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> AppletCodeResult {
    let options = parse_args(args)?;
    let user = lookup_user(&options.user)?;

    if unsafe { libc::geteuid() } != 0 && options.user != current_user_name() {
        authenticate(&options.user)?;
    }

    let status = spawn_shell(&options, &user)
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", None, err)])?;
    Ok(exit_code(status))
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut login = false;
    let mut preserve_env = false;
    let mut shell = None;
    let mut command = None;
    let mut operands = Vec::new();

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => {
                if value == "-" && operands.is_empty() && command.is_none() {
                    login = true;
                } else {
                    operands.push(value.to_string());
                }
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'l' => login = true,
                        'm' | 'p' => preserve_env = true,
                        's' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            shell = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "s")?
                                    .to_string(),
                            );
                            break;
                        }
                        'c' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            command = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "c")?
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

    let (user, extra_args) = if operands.is_empty() {
        (String::from("root"), Vec::new())
    } else {
        (operands[0].clone(), operands[1..].to_vec())
    };

    Ok(Options {
        login,
        preserve_env,
        shell,
        command,
        user,
        extra_args,
    })
}

fn lookup_user(name: &str) -> Result<UserInfo, Vec<AppletError>> {
    let name_c = CString::new(name)
        .map_err(|_| vec![AppletError::new(APPLET, format!("unknown user {name}"))])?;
    let passwd = unsafe { libc::getpwnam(name_c.as_ptr()) };
    if passwd.is_null() {
        return Err(vec![AppletError::new(APPLET, format!("unknown user {name}"))]);
    }

    let entry = unsafe { &*passwd };
    Ok(UserInfo {
        name: name.to_string(),
        uid: entry.pw_uid,
        gid: entry.pw_gid,
        home: c_string_field(entry.pw_dir).unwrap_or_else(|| String::from("/")),
        shell: c_string_field(entry.pw_shell).unwrap_or_else(|| String::from(DEFAULT_SHELL)),
    })
}

fn c_string_field(ptr: *const libc::c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }
}

fn current_user_name() -> String {
    let uid = unsafe { libc::geteuid() };
    let passwd = unsafe { libc::getpwuid(uid) };
    if passwd.is_null() {
        return String::from("root");
    }
    c_string_field(unsafe { (*passwd).pw_name }).unwrap_or_else(|| String::from("root"))
}

fn authenticate(user: &str) -> Result<(), Vec<AppletError>> {
    let paths = account_paths();
    let shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;
    let hash = shadow.iter().find_map(|record| match record {
        ShadowRecord::Entry(entry) if entry.username == user => Some(entry.password.as_str()),
        _ => None,
    });

    let Some(hash) = hash else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown user {user}"),
        )]);
    };

    let mut out = io::stdout().lock();
    write!(out, "Password: ").map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let mut input = BufReader::new(io::stdin().lock());
    let mut password = String::new();
    input
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

fn spawn_shell(options: &Options, user: &UserInfo) -> io::Result<ExitStatus> {
    let shell = options
        .shell
        .clone()
        .unwrap_or_else(|| default_shell(user));

    let mut command = if let Some(script) = &options.command {
        let mut command = Command::new(&shell);
        command.arg("-c").arg(script);
        for arg in &options.extra_args {
            command.arg(arg);
        }
        command
    } else if !options.extra_args.is_empty() {
        let mut command = Command::new(&options.extra_args[0]);
        for arg in &options.extra_args[1..] {
            command.arg(arg);
        }
        command
    } else {
        Command::new(&shell)
    };

    if options.login {
        command.arg0(format!("-{}", login_name(&shell)));
    }

    if options.login {
        command.env_clear();
        if let Ok(term) = env::var("TERM") {
            command.env("TERM", term);
        }
        command.env("PATH", DEFAULT_PATH);
    }

    if !options.preserve_env {
        command.env("HOME", &user.home);
        command.env("SHELL", &shell);
        command.env("USER", &user.name);
        command.env("LOGNAME", &user.name);
        command.env("PATH", env::var("PATH").unwrap_or_else(|_| String::from(DEFAULT_PATH)));
    }

    command.uid(user.uid);
    command.gid(user.gid);
    let home = user.home.clone();
    let login = options.login;
    unsafe {
        command.pre_exec(move || {
            if login {
                env::set_current_dir(&home)?;
            }
            Ok(())
        });
    }

    command.status()
}

fn default_shell(user: &UserInfo) -> String {
    if user.shell.is_empty() {
        String::from(DEFAULT_SHELL)
    } else {
        user.shell.clone()
    }
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
    fn parses_login_mode() {
        let parsed = parse_args(&args(&["-", "root", "-c", "id -un"])).unwrap();
        assert!(parsed.login);
        assert_eq!(parsed.user, "root");
        assert_eq!(parsed.command.as_deref(), Some("id -un"));
    }

    #[test]
    fn parses_preserve_env_mode() {
        let parsed = parse_args(&args(&["-m", "-s", "/bin/sh", "root"])).unwrap();
        assert!(parsed.preserve_env);
        assert_eq!(parsed.shell.as_deref(), Some("/bin/sh"));
        assert_eq!(parsed.user, "root");
    }
}
