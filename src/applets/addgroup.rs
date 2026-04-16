use crate::common::account::{
    GroupEntry, GroupRecord, NORMAL_ID_START, SYSTEM_ID_START, account_paths, find_group,
    find_group_mut, find_passwd, next_available_gid, read_group, read_passwd, write_group,
};
use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "addgroup";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Create {
        name: String,
        gid: Option<u32>,
        system: bool,
    },
    AddUser {
        user: String,
        group: String,
    },
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let command = parse_args(args)?;
    let paths = account_paths();
    let passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let mut group = read_group(&paths.group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("group"), err)])?;

    match command {
        Command::Create { name, gid, system } => create_group(&mut group, &name, gid, system)?,
        Command::AddUser {
            user,
            group: group_name,
        } => add_user_to_group(&passwd, &mut group, &user, &group_name)?,
    }

    write_group(&paths.group, &group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("group"), err)])
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Command, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut gid = None;
    let mut system = false;
    let mut operands = Vec::new();

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => operands.push(value.to_string()),
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'g' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "g")?;
                            gid = Some(parse_id(value, "gid")?);
                            break;
                        }
                        'S' => system = true,
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    match operands.as_slice() {
        [group] => Ok(Command::Create {
            name: group.clone(),
            gid,
            system,
        }),
        [user, group] => {
            if gid.is_some() || system {
                return Err(vec![AppletError::new(APPLET, "extra operand")]);
            }
            Ok(Command::AddUser {
                user: user.clone(),
                group: group.clone(),
            })
        }
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn create_group(
    group: &mut Vec<GroupRecord>,
    name: &str,
    requested_gid: Option<u32>,
    system: bool,
) -> AppletResult {
    if find_group(group, name).is_some() {
        return Err(vec![AppletError::new(
            APPLET,
            format!("group '{name}' in use"),
        )]);
    }

    let gid = match requested_gid {
        Some(gid) => {
            if group.iter().any(|record| matches!(record, GroupRecord::Entry(entry) if entry.gid == gid))
            {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("gid '{gid}' in use"),
                )]);
            }
            gid
        }
        None => next_available_gid(group, if system { SYSTEM_ID_START } else { NORMAL_ID_START }),
    };

    group.push(GroupRecord::Entry(GroupEntry {
        name: name.to_string(),
        password: String::from("x"),
        gid,
        members: Vec::new(),
    }));
    Ok(())
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

fn parse_id(value: &str, kind: &str) -> Result<u32, Vec<AppletError>> {
    value.parse::<u32>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid {kind} '{value}'"),
        )]
    })
}

#[cfg(test)]
mod tests {
    use super::{Command, add_user_to_group, create_group, parse_args};
    use crate::common::account::{GroupEntry, GroupRecord, PasswdEntry, PasswdRecord};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn passwd_record(name: &str) -> PasswdRecord {
        PasswdRecord::Entry(PasswdEntry {
            username: name.to_string(),
            password: String::new(),
            uid: 0,
            gid: 0,
            gecos: String::new(),
            home: String::new(),
            shell: String::new(),
        })
    }

    fn group_record(name: &str, gid: u32) -> GroupRecord {
        GroupRecord::Entry(GroupEntry {
            name: name.to_string(),
            password: String::from("x"),
            gid,
            members: Vec::new(),
        })
    }

    #[test]
    fn parses_create_mode() {
        assert_eq!(
            parse_args(&args(&["-S", "-g", "123", "staff"])).unwrap(),
            Command::Create {
                name: String::from("staff"),
                gid: Some(123),
                system: true,
            }
        );
    }

    #[test]
    fn parses_add_user_mode() {
        assert_eq!(
            parse_args(&args(&["alice", "staff"])).unwrap(),
            Command::AddUser {
                user: String::from("alice"),
                group: String::from("staff"),
            }
        );
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(parse_args(&args(&["-z"])).is_err());
    }

    #[test]
    fn duplicate_members_are_ignored() {
        let passwd = vec![passwd_record("alice")];
        let mut group = vec![group_record("staff", 1000)];
        add_user_to_group(&passwd, &mut group, "alice", "staff").unwrap();
        add_user_to_group(&passwd, &mut group, "alice", "staff").unwrap();

        match &group[0] {
            GroupRecord::Entry(entry) => assert_eq!(entry.members, vec![String::from("alice")]),
            GroupRecord::Raw(_) => panic!("expected group entry"),
        }
    }

    #[test]
    fn create_group_rejects_duplicate_gid() {
        let mut group = vec![group_record("staff", 1000)];
        assert!(create_group(&mut group, "devs", Some(1000), false).is_err());
    }
}
