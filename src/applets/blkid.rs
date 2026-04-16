use std::fs;

use crate::common::applet::finish;
use crate::common::args::argv_to_strings;
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
const EXT_SUPER_OFFSET: u64 = 1024;
const EXT_SUPER_SIZE: usize = 0x200;
const EXT_MAGIC_OFFSET: usize = 0x38;
const EXT_FEATURE_COMPAT_OFFSET: usize = 0x5c;
const EXT_FEATURE_INCOMPAT_OFFSET: usize = 0x60;
const EXT_FEATURE_RO_COMPAT_OFFSET: usize = 0x64;
const EXT_UUID_OFFSET: usize = 0x68;
const EXT_LABEL_OFFSET: usize = 0x78;
const EXT3_FEATURE_HAS_JOURNAL: u32 = 0x0004;
const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32 = 0x0008;
const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
const VFAT_BOOT_SIZE: usize = 512;
const VFAT_SERIAL16_OFFSET: usize = 39;
const VFAT_LABEL16_OFFSET: usize = 43;
const VFAT_SERIAL32_OFFSET: usize = 67;
const VFAT_LABEL32_OFFSET: usize = 71;
const XFS_SUPER_SIZE: usize = 0x200;
const XFS_UUID_OFFSET: usize = 0x20;
const XFS_LABEL_OFFSET: usize = 0x6c;
const BTRFS_SUPER_OFFSET: u64 = 64 * 1024;
const BTRFS_SUPER_SIZE: usize = 0x200;
const BTRFS_FSID_OFFSET: usize = 0x20;
const BTRFS_MAGIC_OFFSET: usize = 0x40;
const ISO9660_PVD_OFFSET: u64 = 16 * 2048;
const ISO9660_PVD_SIZE: usize = 2048;
const ISO9660_MAGIC_OFFSET: usize = 1;
const ISO9660_LABEL_OFFSET: usize = 40;
const ISO9660_LABEL_SIZE: usize = 32;
const SQUASHFS_SUPER_SIZE: usize = 4;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Entry {
    path: String,
    uuid: Option<String>,
    label: Option<String>,
    fstype: Option<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let operands = parse_args(&args)?;
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
        let entry = entries
            .entry(path.clone())
            .or_insert_with(|| base_entry(&path, &tags));
        if entry.fstype.is_none() {
            entry.fstype = Some(mount.filesystem);
        }
        maybe_probe_entry(Path::new(&path), entry);
    }

    for (path, fstype) in proc_swaps().map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some("/proc/swaps"),
            err,
        )]
    })? {
        let path = normalize_device_path(&path);
        let entry = entries
            .entry(path.clone())
            .or_insert_with(|| base_entry(&path, &tags));
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
        maybe_probe_entry(Path::new(&path), entry);
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
    maybe_probe_entry(Path::new(&path), &mut entry);

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
    read_swap_tags_from_file(path, &metadata, &mut file)
}

fn read_swap_tags_from_file(
    path: &Path,
    metadata: &fs::Metadata,
    file: &mut fs::File,
) -> Option<Entry> {
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

    let uuid = read_swap_uuid(file);
    let label = read_swap_label(file);
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid,
        label,
        fstype: Some(String::from("swap")),
    })
}

fn maybe_probe_entry(path: &Path, entry: &mut Entry) {
    if entry.uuid.is_some() && entry.label.is_some() && entry.fstype.is_some() {
        return;
    }
    let Some(probed) = probe_entry(path) else {
        return;
    };
    if entry.uuid.is_none() {
        entry.uuid = probed.uuid;
    }
    if entry.label.is_none() {
        entry.label = probed.label;
    }
    if entry.fstype.is_none() {
        entry.fstype = probed.fstype;
    }
}

fn probe_entry(path: &Path) -> Option<Entry> {
    let metadata = fs::metadata(path).ok()?;
    if !(metadata.file_type().is_file() || metadata.file_type().is_block_device()) {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    read_swap_tags_from_file(path, &metadata, &mut file)
        .or_else(|| read_ext_tags(path, &mut file))
        .or_else(|| read_xfs_tags(path, &mut file))
        .or_else(|| read_vfat_tags(path, &mut file))
        .or_else(|| read_btrfs_tags(path, &mut file))
        .or_else(|| read_iso9660_tags(path, &mut file))
        .or_else(|| read_squashfs_tags(path, &mut file))
}

fn read_ext_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut superblock = [0_u8; EXT_SUPER_SIZE];
    read_at(file, EXT_SUPER_OFFSET, &mut superblock)?;
    if u16::from_le_bytes(
        superblock[EXT_MAGIC_OFFSET..EXT_MAGIC_OFFSET + 2]
            .try_into()
            .ok()?,
    ) != 0xef53
    {
        return None;
    }
    let feature_compat = read_u32_le(&superblock, EXT_FEATURE_COMPAT_OFFSET)?;
    let feature_incompat = read_u32_le(&superblock, EXT_FEATURE_INCOMPAT_OFFSET)?;
    let feature_ro_compat = read_u32_le(&superblock, EXT_FEATURE_RO_COMPAT_OFFSET)?;
    let fstype = if feature_ro_compat
        & (EXT4_FEATURE_RO_COMPAT_HUGE_FILE | EXT4_FEATURE_RO_COMPAT_DIR_NLINK)
        != 0
        || feature_incompat & (EXT4_FEATURE_INCOMPAT_EXTENTS | EXT4_FEATURE_INCOMPAT_64BIT) != 0
    {
        "ext4"
    } else if feature_compat & EXT3_FEATURE_HAS_JOURNAL != 0 {
        "ext3"
    } else {
        "ext2"
    };
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: format_uuid(
            superblock[EXT_UUID_OFFSET..EXT_UUID_OFFSET + 16]
                .try_into()
                .ok()?,
        ),
        label: read_c_string(&superblock[EXT_LABEL_OFFSET..EXT_LABEL_OFFSET + 16]),
        fstype: Some(String::from(fstype)),
    })
}

fn read_xfs_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut superblock = [0_u8; XFS_SUPER_SIZE];
    read_at(file, 0, &mut superblock)?;
    if &superblock[..4] != b"XFSB" {
        return None;
    }
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: format_uuid(
            superblock[XFS_UUID_OFFSET..XFS_UUID_OFFSET + 16]
                .try_into()
                .ok()?,
        ),
        label: read_c_string(&superblock[XFS_LABEL_OFFSET..XFS_LABEL_OFFSET + 12]),
        fstype: Some(String::from("xfs")),
    })
}

fn read_vfat_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut boot = [0_u8; VFAT_BOOT_SIZE];
    read_at(file, 0, &mut boot)?;
    if &boot[3..7] == b"NTFS" || boot[510] != 0x55 || boot[511] != 0xaa {
        return None;
    }
    let (serial_offset, label_offset) = if &boot[82..90] == b"FAT32   " || &boot[54..59] == b"MSDOS"
    {
        (VFAT_SERIAL32_OFFSET, VFAT_LABEL32_OFFSET)
    } else if &boot[54..62] == b"FAT16   " || &boot[54..62] == b"FAT12   " {
        (VFAT_SERIAL16_OFFSET, VFAT_LABEL16_OFFSET)
    } else {
        return None;
    };
    let serial = u32::from_le_bytes(boot[serial_offset..serial_offset + 4].try_into().ok()?);
    let label = read_trimmed_label(&boot[label_offset..label_offset + 11])
        .filter(|value| value != "NO NAME");
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: Some(format!("{:04X}-{:04X}", serial >> 16, serial & 0xffff)),
        label,
        fstype: Some(String::from("vfat")),
    })
}

fn read_btrfs_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut superblock = [0_u8; BTRFS_SUPER_SIZE];
    read_at(file, BTRFS_SUPER_OFFSET, &mut superblock)?;
    if &superblock[BTRFS_MAGIC_OFFSET..BTRFS_MAGIC_OFFSET + 8] != b"_BHRfS_M" {
        return None;
    }
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: format_uuid(
            superblock[BTRFS_FSID_OFFSET..BTRFS_FSID_OFFSET + 16]
                .try_into()
                .ok()?,
        ),
        label: None,
        fstype: Some(String::from("btrfs")),
    })
}

fn read_iso9660_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut descriptor = [0_u8; ISO9660_PVD_SIZE];
    read_at(file, ISO9660_PVD_OFFSET, &mut descriptor)?;
    if descriptor[0] != 1
        || &descriptor[ISO9660_MAGIC_OFFSET..ISO9660_MAGIC_OFFSET + 5] != b"CD001"
        || descriptor[6] != 1
    {
        return None;
    }
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: None,
        label: read_trimmed_label(
            &descriptor[ISO9660_LABEL_OFFSET..ISO9660_LABEL_OFFSET + ISO9660_LABEL_SIZE],
        ),
        fstype: Some(String::from("iso9660")),
    })
}

fn read_squashfs_tags(path: &Path, file: &mut fs::File) -> Option<Entry> {
    let mut superblock = [0_u8; SQUASHFS_SUPER_SIZE];
    read_at(file, 0, &mut superblock)?;
    if &superblock != b"hsqs" {
        return None;
    }
    Some(Entry {
        path: path.to_string_lossy().into_owned(),
        uuid: None,
        label: None,
        fstype: Some(String::from("squashfs")),
    })
}

fn read_at(file: &mut fs::File, offset: u64, buf: &mut [u8]) -> Option<()> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(buf).ok()?;
    Some(())
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn format_uuid(bytes: [u8; 16]) -> Option<String> {
    if bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

fn read_c_string(bytes: &[u8]) -> Option<String> {
    let len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    (len > 0).then(|| {
        String::from_utf8_lossy(&bytes[..len])
            .trim_end()
            .to_string()
    })
}

fn read_trimmed_label(bytes: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(bytes).trim().to_string();
    (!value.is_empty()).then_some(value)
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
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

fn read_swap_label(file: &mut fs::File) -> Option<String> {
    let mut bytes = [0_u8; 16];
    file.seek(SeekFrom::Start(SWAP_LABEL_OFFSET)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    let len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
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
    use super::{
        EXT_FEATURE_INCOMPAT_OFFSET, EXT_LABEL_OFFSET, EXT_MAGIC_OFFSET, EXT_SUPER_OFFSET,
        EXT_UUID_OFFSET, EXT4_FEATURE_INCOMPAT_EXTENTS, Entry, ISO9660_LABEL_OFFSET,
        ISO9660_LABEL_SIZE, ISO9660_PVD_OFFSET, SQUASHFS_SUPER_SIZE, format_entry, parse_args,
        probe_entry, read_state_entries,
    };
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_operands() {
        assert_eq!(
            parse_args(&args(&["/dev/sda1"])).unwrap(),
            vec![String::from("/dev/sda1")]
        );
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

    #[test]
    fn probes_ext4_image() {
        let path = std::env::temp_dir().join(format!("seed-blkid-ext4-{}", std::process::id()));
        let mut image = vec![0_u8; 4096];
        image[EXT_SUPER_OFFSET as usize + EXT_MAGIC_OFFSET
            ..EXT_SUPER_OFFSET as usize + EXT_MAGIC_OFFSET + 2]
            .copy_from_slice(&0xef53_u16.to_le_bytes());
        image[EXT_SUPER_OFFSET as usize + EXT_FEATURE_INCOMPAT_OFFSET
            ..EXT_SUPER_OFFSET as usize + EXT_FEATURE_INCOMPAT_OFFSET + 4]
            .copy_from_slice(&EXT4_FEATURE_INCOMPAT_EXTENTS.to_le_bytes());
        image[EXT_SUPER_OFFSET as usize + EXT_UUID_OFFSET
            ..EXT_SUPER_OFFSET as usize + EXT_UUID_OFFSET + 16]
            .copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        image[EXT_SUPER_OFFSET as usize + EXT_LABEL_OFFSET
            ..EXT_SUPER_OFFSET as usize + EXT_LABEL_OFFSET + 4]
            .copy_from_slice(b"root");
        fs::write(&path, image).unwrap();
        let entry = probe_entry(&path).unwrap();
        assert_eq!(entry.fstype.as_deref(), Some("ext4"));
        assert_eq!(entry.label.as_deref(), Some("root"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn probes_vfat_image() {
        let path = std::env::temp_dir().join(format!("seed-blkid-vfat-{}", std::process::id()));
        let mut image = vec![0_u8; 1024];
        image[54..62].copy_from_slice(b"FAT16   ");
        image[39..43].copy_from_slice(&0x1234_abcd_u32.to_le_bytes());
        image[43..54].copy_from_slice(b"EFI        ");
        image[510] = 0x55;
        image[511] = 0xaa;
        fs::write(&path, image).unwrap();
        let entry = probe_entry(&path).unwrap();
        assert_eq!(entry.fstype.as_deref(), Some("vfat"));
        assert_eq!(entry.label.as_deref(), Some("EFI"));
        assert_eq!(entry.uuid.as_deref(), Some("1234-ABCD"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn probes_iso9660_image() {
        let path = std::env::temp_dir().join(format!("seed-blkid-iso9660-{}", std::process::id()));
        let mut image = vec![0_u8; 20 * 2048];
        let base = ISO9660_PVD_OFFSET as usize;
        image[base] = 1;
        image[base + 1..base + 6].copy_from_slice(b"CD001");
        image[base + 6] = 1;
        image[base + ISO9660_LABEL_OFFSET..base + ISO9660_LABEL_OFFSET + ISO9660_LABEL_SIZE]
            .copy_from_slice(b"SEED_INSTALL                    ");
        fs::write(&path, image).unwrap();
        let entry = probe_entry(&path).unwrap();
        assert_eq!(entry.fstype.as_deref(), Some("iso9660"));
        assert_eq!(entry.label.as_deref(), Some("SEED_INSTALL"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn probes_squashfs_image() {
        let path = std::env::temp_dir().join(format!("seed-blkid-squashfs-{}", std::process::id()));
        let mut image = vec![0_u8; 4096];
        image[..SQUASHFS_SUPER_SIZE].copy_from_slice(b"hsqs");
        fs::write(&path, image).unwrap();
        let entry = probe_entry(&path).unwrap();
        assert_eq!(entry.fstype.as_deref(), Some("squashfs"));
        let _ = fs::remove_file(path);
    }
}
