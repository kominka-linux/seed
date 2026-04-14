use std::env;
use std::ffi::{CStr, CString};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus};

use crate::common::account::{ShadowRecord, account_paths, find_passwd, read_passwd, read_shadow};
use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::error::AppletError;

const APPLET: &str = "sulogin";
const DEFAULT_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> AppletCodeResult {
    let (force, _tty) = parse_args(args)?;
    let paths = account_paths();
    let passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;

    let root = find_passwd(&passwd, "root")
        .cloned()
        .ok_or_else(|| vec![AppletError::new(APPLET, "unknown user root")])?;
    let hash = shadow.iter().find_map(|record| match record {
        ShadowRecord::Entry(entry) if entry.username == "root" => Some(entry.password.as_str()),
        _ => None,
    });

    if !can_skip_auth(force, hash) {
        match prompt_for_password(hash.unwrap_or(""))? {
            AuthResult::Authenticated => {}
            AuthResult::ContinueBoot => return Ok(0),
        }
    }

    let status = spawn_root_shell(&root)
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", None, err)])?;
    Ok(status.code().unwrap_or(1))
}

fn parse_args(args: &[String]) -> Result<(bool, Option<String>), Vec<AppletError>> {
    let mut force = false;
    let mut tty = None;

    for arg in args {
        match arg.as_str() {
            "-e" | "--force" => force = true,
            value if value.starts_with('-') => {
                return Err(vec![AppletError::unrecognized_option(APPLET, value)])
            }
            value => {
                if tty.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                tty = Some(value.to_string());
            }
        }
    }

    Ok((force, tty))
}

fn can_skip_auth(force: bool, hash: Option<&str>) -> bool {
    force && hash.map(|value| value.starts_with('!') || value.starts_with('*')).unwrap_or(true)
}

enum AuthResult {
    Authenticated,
    ContinueBoot,
}

fn prompt_for_password(hash: &str) -> Result<AuthResult, Vec<AppletError>> {
    let mut out = io::stdout().lock();
    write!(
        out,
        "Give root password for system maintenance\n(or press Control-D to continue): "
    )
    .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let mut password = String::new();
    let read = BufReader::new(io::stdin().lock())
        .read_line(&mut password)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
    if read == 0 {
        return Ok(AuthResult::ContinueBoot);
    }
    while matches!(password.as_bytes().last(), Some(b'\n' | b'\r')) {
        password.pop();
    }

    if verify_password(&password, hash) {
        Ok(AuthResult::Authenticated)
    } else {
        Err(vec![AppletError::new(APPLET, "incorrect password")])
    }
}

fn verify_password(password: &str, hash: &str) -> bool {
    if hash.starts_with('!') || hash.starts_with('*') {
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

fn spawn_root_shell(root: &crate::common::account::PasswdEntry) -> io::Result<ExitStatus> {
    let shell = if root.shell.is_empty() {
        String::from("/bin/sh")
    } else {
        root.shell.clone()
    };
    let mut command = Command::new(&shell);
    command.arg0(format!("-{}", shell.rsplit('/').next().unwrap_or(&shell)));
    command.uid(root.uid);
    command.gid(root.gid);
    command.env_clear();
    command.env("HOME", &root.home);
    command.env("SHELL", &shell);
    command.env("USER", "root");
    command.env("LOGNAME", "root");
    command.env("PATH", env::var("PATH").unwrap_or_else(|_| String::from(DEFAULT_PATH)));

    let home = root.home.clone();
    unsafe {
        command.pre_exec(move || {
            env::set_current_dir(&home)?;
            Ok(())
        });
    }

    command.status()
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_force_flag() {
        assert_eq!(parse_args(&args(&["--force", "/dev/console"])).unwrap(), (true, Some(String::from("/dev/console"))));
    }
}
