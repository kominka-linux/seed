use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::account::{
    ShadowEntry, ShadowRecord, account_paths, find_passwd, find_shadow_mut, read_passwd,
    read_shadow, write_shadow,
};
use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::unix::lookup_user;

const APPLET: &str = "passwd";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Algorithm {
    Des,
    Md5,
    Sha256,
    Sha512,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    SetPassword,
    Delete,
    Lock,
    Unlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    algorithm: Algorithm,
    action: Action,
    user: Option<String>,
}

unsafe extern "C" {
    fn crypt(key: *const libc::c_char, salt: *const libc::c_char) -> *mut libc::c_char;
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    let paths = account_paths();
    let passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let mut shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;
    let user = determine_user(&options, &passwd)?;

    let password = match options.action {
        Action::SetPassword => read_new_password(&user, options.algorithm)?,
        Action::Delete => String::new(),
        Action::Lock => lock_password(current_password(&shadow, &user).unwrap_or("")),
        Action::Unlock => unlock_password(current_password(&shadow, &user).unwrap_or("")),
    };

    let last_change = days_since_epoch().to_string();
    if let Some(entry) = find_shadow_mut(&mut shadow, &user) {
        entry.password = password;
        ensure_shadow_defaults(entry, &last_change);
    } else {
        shadow.push(ShadowRecord::Entry(ShadowEntry {
            username: user.clone(),
            password,
            rest: vec![
                last_change,
                String::from("0"),
                String::from("99999"),
                String::from("7"),
                String::new(),
                String::new(),
                String::new(),
            ],
        }));
    }

    write_shadow(&paths.shadow, &shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("shadow"), err)])
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options {
        algorithm: Algorithm::Sha512,
        action: Action::SetPassword,
        user: None,
    };

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => {
                if options.user.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                options.user = Some(value.to_string());
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'a' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "a")?;
                            options.algorithm = parse_algorithm(value)?;
                            break;
                        }
                        'd' => options.action = Action::Delete,
                        'l' => options.action = Action::Lock,
                        'u' => options.action = Action::Unlock,
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    Ok(options)
}

fn parse_algorithm(value: &str) -> Result<Algorithm, Vec<AppletError>> {
    match value {
        "des" => Ok(Algorithm::Des),
        "md5" => Ok(Algorithm::Md5),
        "sha256" => Ok(Algorithm::Sha256),
        "sha512" => Ok(Algorithm::Sha512),
        _ => Err(vec![AppletError::new(APPLET, format!("invalid algorithm '{value}'"))]),
    }
}

fn determine_user(
    options: &Options,
    passwd: &[crate::common::account::PasswdRecord],
) -> Result<String, Vec<AppletError>> {
    let user = options.user.clone().unwrap_or_else(current_user);
    if find_passwd(passwd, &user).is_none() {
        return Err(vec![AppletError::new(APPLET, format!("unknown user {user}"))]);
    }
    Ok(user)
}

fn current_user() -> String {
    let uid = unsafe { libc::geteuid() };
    lookup_user(uid)
}

fn current_password<'a>(shadow: &'a [ShadowRecord], user: &str) -> Option<&'a str> {
    shadow.iter().find_map(|record| match record {
        ShadowRecord::Entry(entry) if entry.username == user => Some(entry.password.as_str()),
        _ => None,
    })
}

fn read_new_password(user: &str, algorithm: Algorithm) -> Result<String, Vec<AppletError>> {
    let mut out = io::stdout().lock();
    writeln!(out, "Changing password for {user}")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    write!(out, "New password: ")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let mut input = BufReader::new(io::stdin().lock());
    let first = read_password_line(&mut input)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;

    writeln!(out)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    write!(out, "Retype password: ")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    let second = read_password_line(&mut input)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
    writeln!(out)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;

    if first != second {
        writeln!(out, "Passwords don't match")
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        return Err(vec![AppletError::new(
            APPLET,
            format!("password for {user} is unchanged"),
        )]);
    }

    hash_password(&first, algorithm)
}

fn read_password_line(reader: &mut dyn BufRead) -> io::Result<String> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "unexpected end of input",
        ));
    }
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(line)
}

fn hash_password(password: &str, algorithm: Algorithm) -> Result<String, Vec<AppletError>> {
    let salt = build_salt(algorithm)?;
    let password = CString::new(password)
        .map_err(|_| vec![AppletError::new(APPLET, "password contains NUL byte")])?;
    let salt_c = CString::new(salt.clone())
        .map_err(|_| vec![AppletError::new(APPLET, "salt contains NUL byte")])?;

    let hashed = unsafe { crypt(password.as_ptr(), salt_c.as_ptr()) };
    if hashed.is_null() {
        return Err(vec![AppletError::new(APPLET, "failed to hash password")]);
    }

    Ok(unsafe { CStr::from_ptr(hashed) }
        .to_string_lossy()
        .into_owned())
}

fn build_salt(algorithm: Algorithm) -> Result<String, Vec<AppletError>> {
    let prefix = match algorithm {
        Algorithm::Des => String::new(),
        Algorithm::Md5 => String::from("$1$"),
        Algorithm::Sha256 => String::from("$5$"),
        Algorithm::Sha512 => String::from("$6$"),
    };
    let length = match algorithm {
        Algorithm::Des => 2,
        _ => 16,
    };
    let body = random_salt(length)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/dev/urandom"), err)])?;
    if matches!(algorithm, Algorithm::Des) {
        Ok(body)
    } else {
        Ok(format!("{prefix}{body}$"))
    }
}

fn random_salt(length: usize) -> io::Result<String> {
    const TABLE: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    let mut bytes = vec![0_u8; length];
    match File::open("/dev/urandom") {
        Ok(mut file) => {
            io::Read::read_exact(&mut file, &mut bytes)?;
        }
        Err(_) => {
            let seed = days_since_epoch() ^ u64::from(std::process::id());
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = seed.wrapping_add(index as u64) as u8;
            }
        }
    }

    Ok(bytes
        .into_iter()
        .map(|byte| TABLE[usize::from(byte) % TABLE.len()] as char)
        .collect())
}

fn ensure_shadow_defaults(entry: &mut ShadowEntry, last_change: &str) {
    if entry.rest.len() < 7 {
        entry.rest.resize(7, String::new());
    }
    entry.rest[0] = last_change.to_string();
    if entry.rest[1].is_empty() {
        entry.rest[1] = String::from("0");
    }
    if entry.rest[2].is_empty() {
        entry.rest[2] = String::from("99999");
    }
    if entry.rest[3].is_empty() {
        entry.rest[3] = String::from("7");
    }
}

fn lock_password(password: &str) -> String {
    if password.starts_with('!') {
        password.to_string()
    } else {
        format!("!{password}")
    }
}

fn unlock_password(password: &str) -> String {
    password.strip_prefix('!').unwrap_or(password).to_string()
}

fn days_since_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400
}

#[cfg(test)]
mod tests {
    use super::{Action, Algorithm, Options, lock_password, parse_args, unlock_password};

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_algorithm_and_user() {
        assert_eq!(
            parse_args(&args(&["-a", "md5", "alice"])).unwrap(),
            Options {
                algorithm: Algorithm::Md5,
                action: Action::SetPassword,
                user: Some(String::from("alice")),
            }
        );
    }

    #[test]
    fn parses_lock_action() {
        assert_eq!(
            parse_args(&args(&["-l", "alice"])).unwrap().action,
            Action::Lock
        );
    }

    #[test]
    fn lock_and_unlock_passwords() {
        assert_eq!(lock_password("hash"), "!hash");
        assert_eq!(unlock_password("!hash"), "hash");
    }
}
