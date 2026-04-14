use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::account::{
    GroupEntry, GroupRecord, NOGROUP_GID, NORMAL_ID_START, SYSTEM_ID_START, ShadowEntry,
    ShadowRecord, account_paths, find_group, find_group_mut, find_passwd, next_available_uid,
    read_group, read_passwd, read_shadow, write_group, write_passwd, write_shadow,
};
use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::fs::{CopyContext, CopyOptions, Dereference, copy_path};

const APPLET: &str = "adduser";
const DEFAULT_SHELL: &str = "/bin/sh";
const DEFAULT_SYSTEM_SHELL: &str = "/sbin/nologin";
const DEFAULT_SKEL: &str = "/etc/skel";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Create(CreateOptions),
    AddToGroup { user: String, group: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CreateOptions {
    username: String,
    home: Option<String>,
    gecos: Option<String>,
    shell: Option<String>,
    group: Option<String>,
    system: bool,
    no_password: bool,
    no_home: bool,
    uid: Option<u32>,
    skel: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let command = parse_args(args)?;
    let paths = account_paths();
    let mut passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let mut group = read_group(&paths.group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("group"), err)])?;
    let mut shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;

    let created_home = match command {
        Command::Create(options) => create_user(&mut passwd, &mut group, &mut shadow, options)?,
        Command::AddToGroup { user, group: name } => {
            add_user_to_group(&passwd, &mut group, &user, &name)?;
            None
        }
    };

    write_group(&paths.group, &group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("group"), err)])?;
    write_passwd(&paths.passwd, &passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("passwd"), err)])?;
    write_shadow(&paths.shadow, &shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("shadow"), err)])?;

    if let Some(home) = created_home {
        create_home(&home.path, home.skel.as_deref(), home.uid, home.gid)
            .map_err(|err| vec![AppletError::from_io(APPLET, "creating", Some(&home.path), err)])?;
    }

    Ok(())
}

fn parse_args(args: &[String]) -> Result<Command, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = CreateOptions {
        username: String::new(),
        home: None,
        gecos: None,
        shell: None,
        group: None,
        system: false,
        no_password: false,
        no_home: false,
        uid: None,
        skel: None,
    };
    let mut operands = Vec::new();

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => operands.push(value.to_string()),
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'h' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.home = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "h")?
                                    .to_string(),
                            );
                            break;
                        }
                        'g' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.gecos = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "g")?
                                    .to_string(),
                            );
                            break;
                        }
                        's' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.shell = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "s")?
                                    .to_string(),
                            );
                            break;
                        }
                        'G' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.group = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "G")?
                                    .to_string(),
                            );
                            break;
                        }
                        'u' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "u")?;
                            options.uid = Some(parse_uid(value)?);
                            break;
                        }
                        'k' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.skel = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "k")?
                                    .to_string(),
                            );
                            break;
                        }
                        'S' => options.system = true,
                        'D' => options.no_password = true,
                        'H' => options.no_home = true,
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    match operands.as_slice() {
        [username] => {
            options.username = username.clone();
            Ok(Command::Create(options))
        }
        [user, group] => {
            if has_create_flags(&options) {
                return Err(vec![AppletError::new(APPLET, "extra operand")]);
            }
            Ok(Command::AddToGroup {
                user: user.clone(),
                group: group.clone(),
            })
        }
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn has_create_flags(options: &CreateOptions) -> bool {
    options.home.is_some()
        || options.gecos.is_some()
        || options.shell.is_some()
        || options.group.is_some()
        || options.system
        || options.no_password
        || options.no_home
        || options.uid.is_some()
        || options.skel.is_some()
}

struct CreatedHome {
    path: String,
    skel: Option<String>,
    uid: u32,
    gid: u32,
}

fn create_user(
    passwd: &mut Vec<crate::common::account::PasswdRecord>,
    group: &mut Vec<GroupRecord>,
    shadow: &mut Vec<ShadowRecord>,
    options: CreateOptions,
) -> Result<Option<CreatedHome>, Vec<AppletError>> {
    if find_passwd(passwd, &options.username).is_some() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("user '{}' in use", options.username),
        )]);
    }

    let uid = match options.uid {
        Some(uid) => {
            if passwd.iter().any(
                |record| matches!(record, crate::common::account::PasswdRecord::Entry(entry) if entry.uid == uid),
            ) {
                return Err(vec![AppletError::new(APPLET, format!("uid '{uid}' in use"))]);
            }
            uid
        }
        None => next_available_uid(
            passwd,
            if options.system {
                SYSTEM_ID_START
            } else {
                NORMAL_ID_START
            },
        ),
    };

    let mut add_to_group = None;
    let gid = if let Some(group_name) = &options.group {
        let entry = find_group(group, group_name).ok_or_else(|| {
            vec![AppletError::new(APPLET, format!("unknown group {group_name}"))]
        })?;
        add_to_group = Some(group_name.clone());
        entry.gid
    } else if options.system {
        NOGROUP_GID
    } else {
        if find_group(group, &options.username).is_some() {
            return Err(vec![AppletError::new(
                APPLET,
                format!("group '{}' in use", options.username),
            )]);
        }
        if group
            .iter()
            .any(|record| matches!(record, GroupRecord::Entry(entry) if entry.gid == uid))
        {
            return Err(vec![AppletError::new(APPLET, format!("gid '{uid}' in use"))]);
        }
        group.push(GroupRecord::Entry(GroupEntry {
            name: options.username.clone(),
            password: String::from("x"),
            gid: uid,
            members: Vec::new(),
        }));
        uid
    };

    if let Some(group_name) = add_to_group {
        let entry = find_group_mut(group, &group_name).expect("group checked above");
        if !entry.members.iter().any(|member| member == &options.username) {
            entry.members.push(options.username.clone());
        }
    }

    let home = options
        .home
        .unwrap_or_else(|| format!("/home/{}", options.username));
    let shell = options.shell.unwrap_or_else(|| {
        if options.system {
            String::from(DEFAULT_SYSTEM_SHELL)
        } else {
            String::from(DEFAULT_SHELL)
        }
    });

    passwd.push(crate::common::account::PasswdRecord::Entry(
        crate::common::account::PasswdEntry {
            username: options.username.clone(),
            password: String::from("x"),
            uid,
            gid,
            gecos: options.gecos.unwrap_or_default(),
            home: home.clone(),
            shell,
        },
    ));
    shadow.push(ShadowRecord::Entry(ShadowEntry {
        username: options.username.clone(),
        password: String::from("!"),
        rest: vec![
            days_since_epoch().to_string(),
            String::from("0"),
            String::from("99999"),
            String::from("7"),
            String::new(),
            String::new(),
            String::new(),
        ],
    }));

    Ok((!options.no_home).then_some(CreatedHome {
        path: home,
        skel: options.skel.or_else(|| Some(String::from(DEFAULT_SKEL))),
        uid,
        gid,
    }))
}

fn add_user_to_group(
    passwd: &[crate::common::account::PasswdRecord],
    group: &mut [GroupRecord],
    user: &str,
    group_name: &str,
) -> AppletResult {
    if find_passwd(passwd, user).is_none() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown user {user}"),
        )]);
    }

    let Some(entry) = find_group_mut(group, group_name) else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown group {group_name}"),
        )]);
    };

    if !entry.members.iter().any(|member| member == user) {
        entry.members.push(user.to_string());
    }

    Ok(())
}

fn create_home(path: &str, skel: Option<&str>, uid: u32, gid: u32) -> io::Result<()> {
    let home = Path::new(path);
    fs::create_dir_all(home)?;

    if let Some(skel) = skel {
        let skel_path = Path::new(skel);
        if skel_path.exists() {
            let mut context = CopyContext::new();
            let options = CopyOptions {
                recursive: true,
                dereference: Dereference::Never,
                preserve_hard_links: false,
                preserve_mode: true,
                preserve_timestamps: true,
            };
            for entry in fs::read_dir(skel_path)? {
                let entry = entry?;
                let destination = home.join(entry.file_name());
                copy_path(&entry.path(), &destination, &options, &mut context, true)
                    .map_err(copy_error_to_io)?;
            }
        }
    }

    chown_recursive(home, uid, gid)
}

fn chown_recursive(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    lchown(path, uid, gid)?;
    if fs::symlink_metadata(path)?.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            chown_recursive(&entry.path(), uid, gid)?;
        }
    }
    Ok(())
}

fn lchown(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))?;
    let rc = unsafe { libc::lchown(path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn copy_error_to_io(err: crate::common::fs::CopyError) -> io::Error {
    match err {
        crate::common::fs::CopyError::Io(err) => err,
        crate::common::fs::CopyError::OmitDirectory => {
            io::Error::other("unexpected directory omission")
        }
    }
}

fn parse_uid(value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .parse::<u32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid uid '{value}'"))])
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
    use super::{Command, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_create_mode() {
        match parse_args(&args(&["-D", "-H", "-G", "staff", "alice"])).unwrap() {
            Command::Create(options) => {
                assert_eq!(options.username, "alice");
                assert_eq!(options.group.as_deref(), Some("staff"));
                assert!(options.no_password);
                assert!(options.no_home);
            }
            Command::AddToGroup { .. } => panic!("expected create mode"),
        }
    }

    #[test]
    fn parses_add_to_group_mode() {
        assert_eq!(
            parse_args(&args(&["alice", "staff"])).unwrap(),
            Command::AddToGroup {
                user: String::from("alice"),
                group: String::from("staff"),
            }
        );
    }

    #[test]
    fn rejects_invalid_uid() {
        assert!(parse_args(&args(&["-u", "bad", "alice"])).is_err());
    }
}
