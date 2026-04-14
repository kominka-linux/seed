use std::collections::HashSet;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::common::fs::AtomicFile;

pub(crate) const NORMAL_ID_START: u32 = 1000;
pub(crate) const SYSTEM_ID_START: u32 = 100;
pub(crate) const NOGROUP_GID: u32 = 65533;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PasswdEntry {
    pub username: String,
    pub password: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: String,
    pub shell: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PasswdRecord {
    Entry(PasswdEntry),
    Raw(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GroupEntry {
    pub name: String,
    pub password: String,
    pub gid: u32,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GroupRecord {
    Entry(GroupEntry),
    Raw(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShadowEntry {
    pub username: String,
    pub password: String,
    pub rest: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShadowRecord {
    Entry(ShadowEntry),
    Raw(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountPaths {
    pub etc_dir: PathBuf,
    pub passwd: PathBuf,
    pub group: PathBuf,
    pub shadow: PathBuf,
}

pub(crate) fn account_paths() -> AccountPaths {
    let etc_dir = env::var_os("SEED_ETC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc"));
    AccountPaths {
        passwd: etc_dir.join("passwd"),
        group: etc_dir.join("group"),
        shadow: etc_dir.join("shadow"),
        etc_dir,
    }
}

pub(crate) fn read_passwd(path: &Path) -> io::Result<Vec<PasswdRecord>> {
    read_records(path, parse_passwd_record)
}

pub(crate) fn write_passwd(path: &Path, records: &[PasswdRecord]) -> io::Result<()> {
    write_records(path, records.iter().map(PasswdRecord::to_line))
}

pub(crate) fn read_group(path: &Path) -> io::Result<Vec<GroupRecord>> {
    read_records(path, parse_group_record)
}

pub(crate) fn write_group(path: &Path, records: &[GroupRecord]) -> io::Result<()> {
    write_records(path, records.iter().map(GroupRecord::to_line))
}

pub(crate) fn read_shadow(path: &Path) -> io::Result<Vec<ShadowRecord>> {
    read_records(path, parse_shadow_record)
}

pub(crate) fn write_shadow(path: &Path, records: &[ShadowRecord]) -> io::Result<()> {
    write_records(path, records.iter().map(ShadowRecord::to_line))
}

pub(crate) fn find_passwd<'a>(
    records: &'a [PasswdRecord],
    username: &str,
) -> Option<&'a PasswdEntry> {
    records.iter().find_map(|record| match record {
        PasswdRecord::Entry(entry) if entry.username == username => Some(entry),
        _ => None,
    })
}

pub(crate) fn find_group<'a>(records: &'a [GroupRecord], name: &str) -> Option<&'a GroupEntry> {
    records.iter().find_map(|record| match record {
        GroupRecord::Entry(entry) if entry.name == name => Some(entry),
        _ => None,
    })
}

pub(crate) fn find_group_mut<'a>(
    records: &'a mut [GroupRecord],
    name: &str,
) -> Option<&'a mut GroupEntry> {
    records.iter_mut().find_map(|record| match record {
        GroupRecord::Entry(entry) if entry.name == name => Some(entry),
        _ => None,
    })
}

#[allow(dead_code)]
pub(crate) fn find_shadow_mut<'a>(
    records: &'a mut [ShadowRecord],
    username: &str,
) -> Option<&'a mut ShadowEntry> {
    records.iter_mut().find_map(|record| match record {
        ShadowRecord::Entry(entry) if entry.username == username => Some(entry),
        _ => None,
    })
}

pub(crate) fn group_has_member(records: &[GroupRecord], name: &str, member: &str) -> bool {
    find_group(records, name)
        .map(|entry| entry.members.iter().any(|value| value == member))
        .unwrap_or(false)
}

pub(crate) fn passwd_entries(records: &[PasswdRecord]) -> impl Iterator<Item = &PasswdEntry> {
    records.iter().filter_map(|record| match record {
        PasswdRecord::Entry(entry) => Some(entry),
        PasswdRecord::Raw(_) => None,
    })
}

pub(crate) fn group_entries(records: &[GroupRecord]) -> impl Iterator<Item = &GroupEntry> {
    records.iter().filter_map(|record| match record {
        GroupRecord::Entry(entry) => Some(entry),
        GroupRecord::Raw(_) => None,
    })
}

pub(crate) fn next_available_uid(records: &[PasswdRecord], start: u32) -> u32 {
    next_available_id(passwd_entries(records).map(|entry| entry.uid), start)
}

pub(crate) fn next_available_gid(records: &[GroupRecord], start: u32) -> u32 {
    next_available_id(group_entries(records).map(|entry| entry.gid), start)
}

fn next_available_id(ids: impl Iterator<Item = u32>, start: u32) -> u32 {
    let used = ids.collect::<HashSet<_>>();
    let mut value = start;
    while used.contains(&value) {
        value = value.saturating_add(1);
    }
    value
}

fn read_records<T>(path: &Path, parse: fn(&str) -> io::Result<T>) -> io::Result<Vec<T>> {
    match fs::read_to_string(path) {
        Ok(text) => text.lines().map(parse).collect(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

fn write_records(lines_path: &Path, lines: impl Iterator<Item = String>) -> io::Result<()> {
    if let Some(parent) = lines_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let existing = fs::metadata(lines_path).ok();
    let mut output = AtomicFile::new(lines_path)?;
    for line in lines {
        output.file_mut().write_all(line.as_bytes())?;
        output.file_mut().write_all(b"\n")?;
    }
    output.commit()?;

    if let Some(metadata) = existing {
        fs::set_permissions(
            lines_path,
            fs::Permissions::from_mode(metadata.mode() & 0o7777),
        )?;
        set_owner(lines_path, metadata.uid(), metadata.gid())?;
    }

    Ok(())
}

fn set_owner(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))?;
    let rc = unsafe { libc::chown(path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn parse_passwd_record(line: &str) -> io::Result<PasswdRecord> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(PasswdRecord::Raw(line.to_string()));
    }

    let parts = line.split(':').collect::<Vec<_>>();
    if parts.len() != 7 {
        return Err(invalid_data("invalid passwd entry"));
    }

    Ok(PasswdRecord::Entry(PasswdEntry {
        username: parts[0].to_string(),
        password: parts[1].to_string(),
        uid: parse_u32(parts[2], "invalid passwd uid")?,
        gid: parse_u32(parts[3], "invalid passwd gid")?,
        gecos: parts[4].to_string(),
        home: parts[5].to_string(),
        shell: parts[6].to_string(),
    }))
}

fn parse_group_record(line: &str) -> io::Result<GroupRecord> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(GroupRecord::Raw(line.to_string()));
    }

    let parts = line.split(':').collect::<Vec<_>>();
    if parts.len() != 4 {
        return Err(invalid_data("invalid group entry"));
    }

    Ok(GroupRecord::Entry(GroupEntry {
        name: parts[0].to_string(),
        password: parts[1].to_string(),
        gid: parse_u32(parts[2], "invalid group gid")?,
        members: split_members(parts[3]),
    }))
}

fn parse_shadow_record(line: &str) -> io::Result<ShadowRecord> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(ShadowRecord::Raw(line.to_string()));
    }

    let parts = line.split(':').collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(invalid_data("invalid shadow entry"));
    }

    Ok(ShadowRecord::Entry(ShadowEntry {
        username: parts[0].to_string(),
        password: parts[1].to_string(),
        rest: parts[2..].iter().map(|value| value.to_string()).collect(),
    }))
}

fn split_members(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(|member| member.to_string()).collect()
    }
}

fn parse_u32(value: &str, message: &'static str) -> io::Result<u32> {
    value.parse::<u32>().map_err(|_| invalid_data(message))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl PasswdRecord {
    fn to_line(&self) -> String {
        match self {
            Self::Entry(entry) => format!(
                "{}:{}:{}:{}:{}:{}:{}",
                entry.username,
                entry.password,
                entry.uid,
                entry.gid,
                entry.gecos,
                entry.home,
                entry.shell
            ),
            Self::Raw(line) => line.clone(),
        }
    }
}

impl GroupRecord {
    fn to_line(&self) -> String {
        match self {
            Self::Entry(entry) => format!(
                "{}:{}:{}:{}",
                entry.name,
                entry.password,
                entry.gid,
                entry.members.join(",")
            ),
            Self::Raw(line) => line.clone(),
        }
    }
}

impl ShadowRecord {
    fn to_line(&self) -> String {
        match self {
            Self::Entry(entry) => {
                let mut fields = vec![entry.username.clone(), entry.password.clone()];
                fields.extend(entry.rest.iter().cloned());
                fields.join(":")
            }
            Self::Raw(line) => line.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GroupRecord, PasswdRecord, ShadowRecord, find_group, next_available_gid,
        next_available_uid, parse_group_record, parse_passwd_record, parse_shadow_record,
        read_group, read_passwd, write_group, write_passwd,
    };
    use crate::common::unix::temp_dir;

    #[test]
    fn parses_records() {
        let passwd = parse_passwd_record("alice:x:1000:1000::/home/alice:/bin/sh").unwrap();
        let group = parse_group_record("alice:x:1000:bob").unwrap();
        let shadow = parse_shadow_record("alice:!:20557:0:99999:7:::").unwrap();
        assert!(matches!(passwd, PasswdRecord::Entry(_)));
        assert!(matches!(group, GroupRecord::Entry(_)));
        assert!(matches!(shadow, ShadowRecord::Entry(_)));
    }

    #[test]
    fn allocates_next_ids() {
        let passwd = vec![
            parse_passwd_record("a:x:1000:1000::/home/a:/bin/sh").unwrap(),
            parse_passwd_record("b:x:1002:1002::/home/b:/bin/sh").unwrap(),
        ];
        let group = vec![
            parse_group_record("a:x:1000:").unwrap(),
            parse_group_record("b:x:1001:").unwrap(),
        ];
        assert_eq!(next_available_uid(&passwd, 1000), 1001);
        assert_eq!(next_available_gid(&group, 1000), 1002);
    }

    #[test]
    fn round_trips_files() {
        let dir = temp_dir("account");
        let group_path = dir.join("group");
        let passwd_path = dir.join("passwd");

        let group = vec![
            GroupRecord::Raw(String::new()),
            parse_group_record("staff:x:1000:alice").unwrap(),
        ];
        let passwd = vec![parse_passwd_record("alice:x:1000:1000::/home/alice:/bin/sh").unwrap()];

        write_group(&group_path, &group).unwrap();
        write_passwd(&passwd_path, &passwd).unwrap();

        let reread_group = read_group(&group_path).unwrap();
        let reread_passwd = read_passwd(&passwd_path).unwrap();
        assert!(find_group(&reread_group, "staff").is_some());
        assert_eq!(reread_group, group);
        assert_eq!(reread_passwd, passwd);
    }
}
