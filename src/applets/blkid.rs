
use std::fs;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::mounts;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

const APPLET: &str = "blkid";
const SWAP_MAGIC: &[u8; 10] = b"SWAPSPACE2";
const SWAP_UUID_OFFSET: u64 = 1024 + 12;
const SWAP_LABEL_OFFSET: u64 = 1024 + 28;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Entry {
    path: String,
    uuid: Option<String>,
    label: Option<String>,
    fstype: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let operands = parse_args(args)?;
    let mut entries = if operands.is_empty() {
        discover_entries()?
    } else {
        operands
            .iter()
            .filter_map(|path| entry_for_path(path))
            .collect::<Vec<_>>()
    };
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    for entry in entries {
        println!("{}", format_entry(&entry));
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Vec<String>, Vec<AppletError>> {
    for arg in args {
        if arg.starts_with('-') {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
    }
    Ok(args.to_vec())
}

fn discover_entries() -> Result<Vec<Entry>, Vec<AppletError>> {
    if let Some(path) = state_path() {
        return read_state_entries(&path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.to_string_lossy()),
                err,
            )]
        });
    }

    let tags = read_device_tags();
    let mut entries = BTreeMap::<String, Entry>::new();

    for mount in mounts::read_mountinfo().map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some("/proc/self/mountinfo"),
            err,
        )]
    })? {
        if !mount.source.starts_with('/') {
            continue;
        }
        let path = normalize_device_path(&mount.source);
        let entry = entries.entry(path.clone()).or_insert_with(|| base_entry(&path, &tags));
        if entry.fstype.is_none() {
            entry.fstype = Some(mount.filesystem);
        }
    }

    for (path, fstype) in proc_swaps().map_err(|err| {
        vec![AppletError::from_io(APPLET, "reading", Some("/proc/swaps"), err)]
    })? {
        let path = normalize_device_path(&path);
        let entry = entries.entry(path.clone()).or_insert_with(|| base_entry(&path, &tags));
        if entry.fstype.is_none() {
            entry.fstype = Some(fstype);
        }
        if (entry.uuid.is_none() || entry.label.is_none())
            && let Some(swap) = read_swap_tags(Path::new(&path))
        {
            entry.uuid.get_or_insert(swap.uuid.unwrap_or_default());
            if entry.uuid.as_deref() == Some("") {
                entry.uuid = None;
            }
            entry.label.get_or_insert(swap.label.unwrap_or_default());
            if entry.label.as_deref() == Some("") {
                entry.label = None;
            }
        }
    }

    Ok(entries.into_values().collect())
}

fn entry_for_path(path: &str) -> Option<Entry> {
    if let Some(state_path) = state_path() {
        return read_state_entries(&state_path).ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| normalize_device_path(&entry.path) == normalize_device_path(path))
        });
    }

    let path = normalize_device_path(path);
    let tags = read_device_tags();
    let mut entry = base_entry(&path, &tags);

    if let Some(mount) = mounts::read_mountinfo()
        .ok()?
        .into_iter()
        .find(|mount| normalize_device_path(&mount.source) == path)
    {
        entry.fstype = Some(mount.filesystem);
    }
    if entry.fstype.is_none()
        && let Some((_, fstype)) = proc_swaps()
            .ok()?
            .into_iter()
            .find(|(device, _)| normalize_device_path(device) == path)
    {
        entry.fstype = Some(fstype);
    }
    if (entry.uuid.is_none() || entry.label.is_none() || entry.fstype.as_deref() == Some("swap"))
        && let Some(swap) = read_swap_tags(Path::new(&path))
    {
        if entry.uuid.is_none() {
            entry.uuid = swap.uuid;
        }
        if entry.label.is_none() {
            entry.label = swap.label;
        }
        if entry.fstype.is_none() {
            entry.fstype = Some(String::from("swap"));
        }
    }

    (entry.uuid.is_some() || entry.label.is_some() || entry.fstype.is_some()).then_some(entry)
}

fn base_entry(path: &str, tags: &BTreeMap<String, DeviceTags>) -> Entry {
    let mut entry = Entry {
        path: path.to_string(),
        ..Entry::default()
    };
    if let Some(tags) = tags.get(path) {
        entry.uuid = tags.uuid.clone();
        entry.label = tags.label.clone();
    }
    entry
}

fn format_entry(entry: &Entry) -> String {
    let mut fields = Vec::new();
    if let Some(uuid) = &entry.uuid {
        fields.push(format!("UUID=\"{uuid}\""));
    }
    if let Some(label) = &entry.label {
        fields.push(format!("LABEL=\"{label}\""));
    }
    if let Some(fstype) = &entry.fstype {
        fields.push(format!("TYPE=\"{fstype}\""));
    }
    format!("{}: {}", entry.path, fields.join(" "))
}

#[derive(Clone, Debug, Default)]
struct DeviceTags {
    uuid: Option<String>,
    label: Option<String>,
}

fn read_device_tags() -> BTreeMap<String, DeviceTags> {
    let mut tags = BTreeMap::<String, DeviceTags>::new();
    load_tag_directory("/dev/disk/by-uuid", true, &mut tags);
    load_tag_directory("/dev/disk/by-label", false, &mut tags);
    tags
}

fn load_tag_directory(path: &str, is_uuid: bool, tags: &mut BTreeMap<String, DeviceTags>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(target) = fs::canonicalize(entry.path()) else {
            continue;
        };
        let key = normalize_device_path(&target.to_string_lossy());
        let value = tags.entry(key).or_default();
        if is_uuid {
            value.uuid = Some(name);
        } else {
            value.label = Some(name);
        }
    }
}

fn proc_swaps() -> std::io::Result<Vec<(String, String)>> {
    let path = std::env::var_os("SEED_PROC_SWAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc/swaps"));
    let text = fs::read_to_string(path)?;
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            (fields.len() >= 2).then(|| (fields[0].to_string(), fields[1].to_string()))
        })
        .collect())
}

fn read_swap_tags(path: &Path) -> Option<Entry> {
    let metadata = fs::metadata(path).ok()?;
    if !(metadata.file_type().is_file() || metadata.file_type().is_block_device()) {
        return None;
    }

    let mut file = fs::File::open(path).ok()?;
    let page_size = page_size()?;
    if metadata.len() < page_size as u64 {
        return None;
    }

    let mut magic = [0_u8; 10];
    file.seek(SeekFrom::Start(page_size as u64 - SWAP_MAGIC.len() as u64))
        .ok()?;
    file.read_exact(&mut magic).ok()?;
    if &magic != SWAP_MAGIC {
        return None;
    }

    let uuid = read_swap_uuid(&mut file);
    let label = read_swap_label(&mut file);
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid,
        label,
        fstype: Some(String::from("swap")),
    })
}

fn read_swap_uuid(file: &mut fs::File) -> Option<String> {
    let mut bytes = [0_u8; 16];
    file.seek(SeekFrom::Start(SWAP_UUID_OFFSET)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    if bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

fn read_swap_label(file: &mut fs::File) -> Option<String> {
    let mut bytes = [0_u8; 16];
    file.seek(SeekFrom::Start(SWAP_LABEL_OFFSET)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    let len = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    (len > 0).then(|| String::from_utf8_lossy(&bytes[..len]).into_owned())
}

fn page_size() -> Option<usize> {
    // SAFETY: sysconf reads process-global configuration and has no side effects.
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (value > 0).then_some(value as usize)
}

fn normalize_device_path(path: &str) -> String {
    fs::canonicalize(path)
        .map(|resolved| resolved.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

fn state_path() -> Option<PathBuf> {
    std::env::var_os("SEED_BLKID_CACHE").map(PathBuf::from)
}

fn read_state_entries(path: &Path) -> std::io::Result<Vec<Entry>> {
    let text = fs::read_to_string(path)?;
    let mut seen = BTreeSet::new();
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\t');
            let path = fields.next()?.to_string();
            if !seen.insert(path.clone()) {
                return None;
            }
            Some(Entry {
                path,
                uuid: fields.next().and_then(nonempty),
                label: fields.next().and_then(nonempty),
                fstype: fields.next().and_then(nonempty),
            })
        })
        .collect())
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Entry, format_entry, parse_args};
    use super::read_state_entries;
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_operands() {
        assert_eq!(parse_args(&args(&["/dev/sda1"])).unwrap(), vec![String::from("/dev/sda1")]);
    }

    #[test]
    fn formats_entries() {
        let line = format_entry(&Entry {
            path: String::from("/dev/sda1"),
            uuid: Some(String::from("u")),
            label: Some(String::from("l")),
            fstype: Some(String::from("ext4")),
        });
        assert_eq!(line, "/dev/sda1: UUID=\"u\" LABEL=\"l\" TYPE=\"ext4\"");
    }

    #[test]
    fn reads_state_cache() {
        let path = std::env::temp_dir().join(format!("seed-blkid-{}", std::process::id()));
        fs::write(&path, "/dev/a\tuuid-a\tlabel-a\text4\n/dev/b\t\t\tswap\n").unwrap();
        let entries = read_state_entries(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].uuid.as_deref(), Some("uuid-a"));
        assert_eq!(entries[1].fstype.as_deref(), Some("swap"));
        let _ = fs::remove_file(path);
    }
}
