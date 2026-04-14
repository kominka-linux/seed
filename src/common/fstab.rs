use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FstabEntry {
    pub(crate) spec: String,
    pub(crate) file: String,
    pub(crate) vfstype: String,
    pub(crate) mntops: Vec<String>,
    pub(crate) freq: u32,
    pub(crate) passno: u32,
}

pub(crate) fn default_path() -> PathBuf {
    env::var_os("SEED_FSTAB")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/fstab"))
}

pub(crate) fn read_entries() -> io::Result<Vec<FstabEntry>> {
    read_entries_from_path(&default_path())
}

pub(crate) fn read_entries_from_path(path: &Path) -> io::Result<Vec<FstabEntry>> {
    let text = fs::read_to_string(path)?;
    Ok(text.lines().filter_map(parse_line).collect())
}

pub(crate) fn parse_line(line: &str) -> Option<FstabEntry> {
    let line = line.split('#').next()?.trim();
    if line.is_empty() {
        return None;
    }

    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 4 {
        return None;
    }

    Some(FstabEntry {
        spec: unescape_field(fields[0]),
        file: unescape_field(fields[1]),
        vfstype: fields[2].to_string(),
        mntops: fields[3]
            .split(',')
            .filter(|option| !option.is_empty())
            .map(str::to_string)
            .collect(),
        freq: fields
            .get(4)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        passno: fields
            .get(5)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
    })
}

pub(crate) fn option_present(options: &[String], target: &str) -> bool {
    options.iter().any(|option| {
        option == target
            || option
                .split_once('=')
                .is_some_and(|(name, _)| name == target)
    })
}

pub(crate) fn option_value<'a>(options: &'a [String], target: &str) -> Option<&'a str> {
    options.iter().find_map(|option| {
        let (name, value) = option.split_once('=')?;
        (name == target).then_some(value)
    })
}

pub(crate) fn matches_type_filter(filter: Option<&[String]>, vfstype: &str) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    if filter.is_empty() {
        return true;
    }

    let excluded = filter
        .iter()
        .filter_map(|item| item.strip_prefix("no"))
        .collect::<Vec<_>>();
    let included = filter
        .iter()
        .filter(|item| !item.starts_with("no"))
        .map(String::as_str)
        .collect::<Vec<_>>();

    if excluded.contains(&vfstype) {
        return false;
    }
    included.is_empty() || included.contains(&vfstype)
}

pub(crate) fn matches_option_filter(filter: Option<&[String]>, options: &[String]) -> bool {
    filter
        .unwrap_or(&[])
        .iter()
        .all(|item| option_present(options, item))
}

fn unescape_field(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &value[index + 1..index + 4];
            if octal
                .as_bytes()
                .iter()
                .all(|byte| (b'0'..=b'7').contains(byte))
                && let Ok(decoded) = u8::from_str_radix(octal, 8)
            {
                result.push(decoded as char);
                index += 4;
                continue;
            }
        }
        result.push(bytes[index] as char);
        index += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        matches_option_filter, matches_type_filter, option_present, option_value, parse_line,
    };

    #[test]
    fn parses_basic_fstab_line() {
        let entry = parse_line("/dev/sda1 / ext4 defaults,noatime 1 1").expect("parse fstab");
        assert_eq!(entry.spec, "/dev/sda1");
        assert_eq!(entry.file, "/");
        assert_eq!(entry.vfstype, "ext4");
        assert_eq!(entry.mntops, vec!["defaults", "noatime"]);
        assert_eq!(entry.freq, 1);
        assert_eq!(entry.passno, 1);
    }

    #[test]
    fn option_helpers_understand_values() {
        let options = vec!["pri=5".to_string(), "discard=pages".to_string()];
        assert!(option_present(&options, "pri"));
        assert_eq!(option_value(&options, "discard"), Some("pages"));
    }

    #[test]
    fn type_filter_supports_negative_entries() {
        let filter = vec!["ext4".to_string(), "novfat".to_string()];
        assert!(matches_type_filter(Some(&filter), "ext4"));
        assert!(!matches_type_filter(Some(&filter), "vfat"));
    }

    #[test]
    fn option_filter_requires_all_requested_options() {
        let filter = vec!["noauto".to_string(), "x-init".to_string()];
        let options = vec![
            "defaults".to_string(),
            "noauto".to_string(),
            "x-init=1".to_string(),
        ];
        assert!(matches_option_filter(Some(&filter), &options));
    }
}
