use crate::common::account::{
    account_paths, find_group, group_has_member, passwd_entries, read_group, read_passwd,
    write_group, GroupRecord,
};
use crate::common::applet::{finish, AppletResult};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "delgroup";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    DeleteGroup { group: String },
    RemoveUser { user: String, group: String },
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    let command = parse_args(&args)?;
    let paths = account_paths();
    let passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let mut group = read_group(&paths.group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("group"), err)])?;

    match command {
        Command::DeleteGroup { group: name } => delete_group(&passwd, &mut group, &name)?,
        Command::RemoveUser { user, group: name } => {
            remove_user_from_group(&mut group, &user, &name)?
        }
    }

    write_group(&paths.group, &group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("group"), err)])
}

fn parse_args(args: &[String]) -> Result<Command, Vec<AppletError>> {
    match args {
        [group] => Ok(Command::DeleteGroup {
            group: group.clone(),
        }),
        [user, group] => Ok(Command::RemoveUser {
            user: user.clone(),
            group: group.clone(),
        }),
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn delete_group(
    passwd: &[crate::common::account::PasswdRecord],
    group: &mut Vec<GroupRecord>,
    name: &str,
) -> AppletResult {
    let Some(entry) = find_group(group, name) else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown group {name}"),
        )]);
    };

    if let Some(user) = passwd_entries(passwd).find(|user| user.gid == entry.gid) {
        return Err(vec![AppletError::new(
            APPLET,
            format!(
                "'{name}' still has '{}' as their primary group!",
                user.username
            ),
        )]);
    }

    group.retain(|record| !matches!(record, GroupRecord::Entry(entry) if entry.name == name));
    Ok(())
}

fn remove_user_from_group(group: &mut [GroupRecord], user: &str, name: &str) -> AppletResult {
    let has_member = group_has_member(group, name, user);
    let Some(entry) = group.iter_mut().find_map(|record| match record {
        GroupRecord::Entry(entry) if entry.name == name => Some(entry),
        _ => None,
    }) else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown group {name}"),
        )]);
    };

    if has_member {
        entry.members.retain(|member| member != user);
    } else {
        eprintln!("{APPLET}: can't find {user} in /etc/group");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_args, Command};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_delete_group_mode() {
        assert_eq!(
            parse_args(&args(&["staff"])).unwrap(),
            Command::DeleteGroup {
                group: String::from("staff"),
            }
        );
    }

    #[test]
    fn parses_remove_user_mode() {
        assert_eq!(
            parse_args(&args(&["alice", "staff"])).unwrap(),
            Command::RemoveUser {
                user: String::from("alice"),
                group: String::from("staff"),
            }
        );
    }
}
