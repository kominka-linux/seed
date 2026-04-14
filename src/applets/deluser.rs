use std::fs;

use crate::common::account::{
    GroupRecord, ShadowRecord, account_paths, find_passwd, passwd_entries, read_group,
    read_passwd, read_shadow, write_group, write_passwd, write_shadow,
};
use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "deluser";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (remove_home, user) = parse_args(args)?;
    let paths = account_paths();
    let mut passwd = read_passwd(&paths.passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("passwd"), err)])?;
    let mut group = read_group(&paths.group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("group"), err)])?;
    let mut shadow = read_shadow(&paths.shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("shadow"), err)])?;

    let user_entry = find_passwd(&passwd, &user)
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("unknown user {user}"))])?
        .clone();

    passwd.retain(|record| {
        !matches!(record, crate::common::account::PasswdRecord::Entry(entry) if entry.username == user)
    });
    shadow.retain(|record| !matches!(record, ShadowRecord::Entry(entry) if entry.username == user));

    let mut removed_from_group = false;
    for record in &mut group {
        if let GroupRecord::Entry(entry) = record {
            let before = entry.members.len();
            entry.members.retain(|member| member != &user);
            if before != entry.members.len() {
                removed_from_group = true;
            }
        }
    }

    let private_group_removed = remove_private_group(&mut group, &user, user_entry.gid, &passwd);
    if !removed_from_group && !private_group_removed {
        eprintln!("{APPLET}: can't find {user} in /etc/group");
    }

    write_group(&paths.group, &group)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("group"), err)])?;
    write_passwd(&paths.passwd, &passwd)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("passwd"), err)])?;
    write_shadow(&paths.shadow, &shadow)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some("shadow"), err)])?;

    if remove_home {
        fs::remove_dir_all(&user_entry.home).map_err(|err| {
            vec![AppletError::from_io(APPLET, "removing", Some(&user_entry.home), err)]
        })?;
    }

    Ok(())
}

fn parse_args(args: &[String]) -> Result<(bool, String), Vec<AppletError>> {
    match args {
        [user] => Ok((false, user.clone())),
        [flag, user] if flag == "--remove-home" => Ok((true, user.clone())),
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn remove_private_group(
    group: &mut Vec<GroupRecord>,
    username: &str,
    gid: u32,
    passwd: &[crate::common::account::PasswdRecord],
) -> bool {
    let Some(index) = group.iter().position(
        |record| matches!(record, GroupRecord::Entry(entry) if entry.name == username && entry.gid == gid),
    ) else {
        return false;
    };

    if passwd_entries(passwd).any(|entry| entry.gid == gid) {
        return false;
    }

    group.remove(index);
    true
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_remove_home_flag() {
        assert_eq!(
            parse_args(&args(&["--remove-home", "alice"])).unwrap(),
            (true, String::from("alice"))
        );
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(parse_args(&args(&["alice", "staff"])).is_err());
    }
}
