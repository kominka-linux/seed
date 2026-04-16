use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use crate::common::applet::finish;
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "fdisk";
const DEFAULT_SECTOR_SIZE: u64 = 512;
const ALIGNMENT_SECTORS: u64 = 2048;
const MBR_SIGNATURE_OFFSET: usize = 510;
const MBR_PARTITION_OFFSET: usize = 446;
const GPT_ENTRY_COUNT: u32 = 128;
const GPT_ENTRY_SIZE: u32 = 128;
const GPT_HEADER_SIZE: u32 = 92;
const BLKRRPART: libc::c_ulong = 0x125f;
const BLKSSZGET: libc::c_ulong = 0x1268;
const BLKGETSIZE64: libc::c_ulong = 0x8008_1272;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    list: bool,
    size: bool,
    sector_size_override: Option<u64>,
    disks: Vec<String>,
}

#[derive(Debug)]
struct Device {
    file: File,
    path: String,
    sector_size: u64,
    total_bytes: u64,
    total_sectors: u64,
    is_block: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DiskLabel {
    None,
    Dos(DosTable),
    Gpt(GptTable),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DosTable {
    partitions: BTreeMap<u32, DosPartition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DosPartition {
    bootable: bool,
    type_code: u8,
    start_lba: u32,
    sector_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GptTable {
    disk_guid: Guid,
    partitions: BTreeMap<u32, GptPartition>,
    entry_count: u32,
    entry_size: u32,
    first_usable_lba: u64,
    last_usable_lba: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GptPartition {
    type_guid: Guid,
    partition_guid: Guid,
    first_lba: u64,
    last_lba: u64,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Guid([u8; 16]);

const GUID_ZERO: Guid = Guid([0; 16]);
const GUID_EFI_SYSTEM: Guid = Guid([
    0xc1, 0x2a, 0x73, 0x28, 0xf8, 0x1f, 0x11, 0xd2, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9,
    0x3b,
]);
const GUID_LINUX_FILESYSTEM: Guid = Guid([
    0x0f, 0xc6, 0x3d, 0xaf, 0x84, 0x83, 0x47, 0x72, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d,
    0xe4,
]);
const GUID_LINUX_SWAP: Guid = Guid([
    0x06, 0x57, 0xfd, 0x6d, 0xa4, 0xab, 0x43, 0xc4, 0x84, 0xe5, 0x09, 0x33, 0xc8, 0x4b, 0x4f,
    0x4f,
]);
const GUID_BIOS_BOOT: Guid = Guid([
    0x21, 0x68, 0x61, 0x48, 0x64, 0x49, 0x6e, 0x6f, 0x74, 0x4e, 0x65, 0x65, 0x64, 0x45, 0x46,
    0x49,
]);

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    if options.list {
        return list_tables(&options);
    }
    if options.size {
        return print_sizes(&options);
    }
    run_interactive(&options)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg(APPLET)? {
        match token {
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.char_indices().peekable();
                while let Some((index, flag)) = chars.next() {
                    match flag {
                        'l' => options.list = true,
                        's' => options.size = true,
                        'u' => {}
                        'b' | 'C' | 'H' | 'S' => {
                            let attached = &flags[index + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, &flag.to_string())?;
                            let parsed = value.parse::<u64>().map_err(|_| {
                                vec![AppletError::new(
                                    APPLET,
                                    format!("invalid number '{value}'"),
                                )]
                            })?;
                            if flag == 'b' {
                                if parsed < 512 || !parsed.is_power_of_two() {
                                    return Err(vec![AppletError::new(
                                        APPLET,
                                        format!("invalid sector size '{value}'"),
                                    )]);
                                }
                                options.sector_size_override = Some(parsed);
                            }
                            break;
                        }
                        _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                    }
                    if chars.peek().is_some() && matches!(flag, 'b' | 'C' | 'H' | 'S') {
                        break;
                    }
                }
            }
            ArgToken::Operand(value) => options.disks.push(value.to_string()),
        }
    }

    if options.list && options.size {
        return Err(vec![AppletError::new(
            APPLET,
            "options '-l' and '-s' are mutually exclusive",
        )]);
    }
    if options.size && options.disks.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }
    if !options.list && !options.size {
        match options.disks.len() {
            1 => {}
            0 => return Err(vec![AppletError::new(APPLET, "missing operand")]),
            _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }

    Ok(options)
}

fn list_tables(options: &Options) -> Result<(), Vec<AppletError>> {
    let disks = if options.disks.is_empty() {
        discover_disk_paths().map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/proc/partitions"), err)])?
    } else {
        options.disks.clone()
    };

    let mut first = true;
    for disk in disks {
        let mut device = open_device(&disk, false, options.sector_size_override)?;
        let label = read_disk_label(&mut device)?;
        if !first {
            println!();
        }
        first = false;
        print_disk(&device, &label);
    }
    Ok(())
}

fn print_sizes(options: &Options) -> Result<(), Vec<AppletError>> {
    for disk in &options.disks {
        let device = open_device(disk, false, options.sector_size_override)?;
        println!("{}", device.total_bytes / 1024);
    }
    Ok(())
}

fn run_interactive(options: &Options) -> Result<(), Vec<AppletError>> {
    let path = &options.disks[0];
    let mut device = open_device(path, true, options.sector_size_override)?;
    let mut label = read_disk_label(&mut device)?;
    if matches!(label, DiskLabel::None) {
        eprintln!("Device contains no recognized partition table");
    }
    let mut reader = CommandReader::new();

    loop {
        let line = reader.read_line("Command (m for help): ")?;
        let command = line.chars().next().unwrap_or('\0');
        match command {
            '\0' => continue,
            'm' => print_help(),
            'p' => print_disk(&device, &label),
            'o' => {
                label = DiskLabel::Dos(DosTable::default());
                eprintln!("Created a new DOS disklabel");
            }
            'g' => {
                label = DiskLabel::Gpt(new_gpt_table(device.total_sectors, device.sector_size)?);
                eprintln!("Created a new GPT disklabel");
            }
            'n' => add_partition(&mut label, &device, &mut reader)?,
            'd' => delete_partition(&mut label, &mut reader)?,
            't' => change_partition_type(&mut label, &mut reader)?,
            'a' => toggle_bootable(&mut label, &mut reader)?,
            'w' => {
                write_disk_label(&mut device, &label)?;
                println!("The partition table has been altered.");
                return Ok(());
            }
            'q' => return Ok(()),
            _ => eprintln!("Unknown command '{command}'"),
        }
    }
}

fn open_device(path: &str, write: bool, sector_size_override: Option<u64>) -> Result<Device, Vec<AppletError>> {
    let file = OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(path), err)])?;
    let metadata = file
        .metadata()
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?;
    let is_block = metadata.file_type().is_block_device();
    let sector_size = sector_size_override
        .or_else(|| is_block.then(|| block_sector_size(file.as_raw_fd())).transpose().ok().flatten())
        .unwrap_or(DEFAULT_SECTOR_SIZE);
    let total_bytes = if is_block {
        block_device_size(file.as_raw_fd()).unwrap_or(metadata.len())
    } else {
        metadata.len()
    };
    if total_bytes < sector_size {
        return Err(vec![AppletError::new(
            APPLET,
            format!("'{}' is too small", path),
        )]);
    }
    Ok(Device {
        file,
        path: path.to_string(),
        sector_size,
        total_bytes,
        total_sectors: total_bytes / sector_size,
        is_block,
    })
}

fn discover_disk_paths() -> io::Result<Vec<String>> {
    let path = std::env::var_os("SEED_PROC_PARTITIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/proc/partitions"));
    let text = fs::read_to_string(path)?;
    let mut disks = Vec::new();
    for line in text.lines().skip(2) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 4 {
            continue;
        }
        let name = fields[3];
        let sys_partition = Path::new("/sys/class/block").join(name).join("partition");
        if sys_partition.exists() {
            continue;
        }
        let device = format!("/dev/{name}");
        if Path::new(&device).exists() {
            disks.push(device);
        }
    }
    Ok(disks)
}

fn read_disk_label(device: &mut Device) -> Result<DiskLabel, Vec<AppletError>> {
    let sector0 = read_lba(device, 0, 1)?;
    if device.total_sectors > 1 {
        let sector1 = read_lba(device, 1, 1)?;
        if sector1.starts_with(b"EFI PART") {
            return parse_gpt_table(device, &sector1);
        }
    }
    if sector0.len() >= MBR_SIGNATURE_OFFSET + 2
        && sector0[MBR_SIGNATURE_OFFSET] == 0x55
        && sector0[MBR_SIGNATURE_OFFSET + 1] == 0xaa
    {
        return Ok(DiskLabel::Dos(parse_dos_table(&sector0)));
    }
    Ok(DiskLabel::None)
}

fn read_lba(device: &mut Device, lba: u64, sectors: u64) -> Result<Vec<u8>, Vec<AppletError>> {
    let byte_offset = lba
        .checked_mul(device.sector_size)
        .ok_or_else(|| vec![AppletError::new(APPLET, "disk offset overflow")])?;
    let byte_count = sectors
        .checked_mul(device.sector_size)
        .ok_or_else(|| vec![AppletError::new(APPLET, "disk size overflow")])?;
    let mut buffer = vec![0_u8; byte_count as usize];
    device
        .file
        .seek(SeekFrom::Start(byte_offset))
        .map_err(|err| vec![AppletError::from_io(APPLET, "seeking", Some(&device.path), err)])?;
    device
        .file
        .read_exact(&mut buffer)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&device.path), err)])?;
    Ok(buffer)
}

fn write_lba(device: &mut Device, lba: u64, data: &[u8]) -> Result<(), Vec<AppletError>> {
    let byte_offset = lba
        .checked_mul(device.sector_size)
        .ok_or_else(|| vec![AppletError::new(APPLET, "disk offset overflow")])?;
    device
        .file
        .seek(SeekFrom::Start(byte_offset))
        .map_err(|err| vec![AppletError::from_io(APPLET, "seeking", Some(&device.path), err)])?;
    device
        .file
        .write_all(data)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&device.path), err)])
}

fn parse_dos_table(sector0: &[u8]) -> DosTable {
    let mut table = DosTable::default();
    for index in 0..4 {
        let offset = MBR_PARTITION_OFFSET + index * 16;
        if offset + 16 > sector0.len() {
            break;
        }
        let bootable = sector0[offset] == 0x80;
        let type_code = sector0[offset + 4];
        let start_lba = u32::from_le_bytes([
            sector0[offset + 8],
            sector0[offset + 9],
            sector0[offset + 10],
            sector0[offset + 11],
        ]);
        let sector_count = u32::from_le_bytes([
            sector0[offset + 12],
            sector0[offset + 13],
            sector0[offset + 14],
            sector0[offset + 15],
        ]);
        if type_code != 0 || sector_count != 0 {
            table.partitions.insert(
                index as u32 + 1,
                DosPartition {
                    bootable,
                    type_code,
                    start_lba,
                    sector_count,
                },
            );
        }
    }
    table
}

fn parse_gpt_table(device: &mut Device, header_sector: &[u8]) -> Result<DiskLabel, Vec<AppletError>> {
    if header_sector.len() < GPT_HEADER_SIZE as usize {
        return Err(vec![AppletError::new(APPLET, "truncated GPT header")]);
    }
    let header_size = read_u32(header_sector, 12) as usize;
    if header_size < GPT_HEADER_SIZE as usize || header_size > header_sector.len() {
        return Err(vec![AppletError::new(APPLET, "invalid GPT header size")]);
    }
    let header_crc = read_u32(header_sector, 16);
    let mut header_bytes = header_sector[..header_size].to_vec();
    header_bytes[16..20].fill(0);
    if crc32(&header_bytes) != header_crc {
        return Err(vec![AppletError::new(APPLET, "invalid GPT header checksum")]);
    }

    let first_usable_lba = read_u64(header_sector, 40);
    let last_usable_lba = read_u64(header_sector, 48);
    let disk_guid = Guid::from_gpt_bytes(&header_sector[56..72]);
    let entries_lba = read_u64(header_sector, 72);
    let entry_count = read_u32(header_sector, 80);
    let entry_size = read_u32(header_sector, 84);
    if entry_count == 0 || entry_size < GPT_ENTRY_SIZE {
        return Err(vec![AppletError::new(APPLET, "invalid GPT entry array")]);
    }
    let entries_bytes = entry_count as usize * entry_size as usize;
    let entries_sectors = div_ceil(entries_bytes as u64, device.sector_size);
    let mut entries = read_lba(device, entries_lba, entries_sectors)?;
    entries.truncate(entries_bytes);
    if crc32(&entries) != read_u32(header_sector, 88) {
        return Err(vec![AppletError::new(APPLET, "invalid GPT entry checksum")]);
    }

    let mut partitions = BTreeMap::new();
    for index in 0..entry_count {
        let offset = index as usize * entry_size as usize;
        let entry = &entries[offset..offset + entry_size as usize];
        let type_guid = Guid::from_gpt_bytes(&entry[0..16]);
        if type_guid == GUID_ZERO {
            continue;
        }
        let partition_guid = Guid::from_gpt_bytes(&entry[16..32]);
        let first_lba = read_u64(entry, 32);
        let last_lba = read_u64(entry, 40);
        let name = decode_gpt_name(&entry[56..128.min(entry.len())]);
        partitions.insert(
            index + 1,
            GptPartition {
                type_guid,
                partition_guid,
                first_lba,
                last_lba,
                name,
            },
        );
    }

    Ok(DiskLabel::Gpt(GptTable {
        disk_guid,
        partitions,
        entry_count,
        entry_size,
        first_usable_lba,
        last_usable_lba,
    }))
}

fn print_disk(device: &Device, label: &DiskLabel) {
    println!(
        "Disk {}: {}, {} bytes, {} sectors",
        device.path,
        human_size(device.total_bytes),
        device.total_bytes,
        device.total_sectors
    );
    println!(
        "Units: sectors of 1 * {} = {} bytes",
        device.sector_size, device.sector_size
    );
    println!("Disklabel type: {}", disk_label_name(label));

    match label {
        DiskLabel::None => {}
        DiskLabel::Dos(table) => {
            println!();
            println!(
                "{:<16} {:<4} {:>10} {:>10} {:>10} {:>8} {:>3} Type",
                "Device", "Boot", "Start", "End", "Sectors", "Size", "Id"
            );
            for (number, partition) in &table.partitions {
                let end = partition.end_lba();
                println!(
                    "{:<16} {:<4} {:>10} {:>10} {:>10} {:>8} {:>02x} {}",
                    partition_path(&device.path, *number),
                    if partition.bootable { "*" } else { "" },
                    partition.start_lba,
                    end,
                    partition.sector_count,
                    human_size(partition.sector_count as u64 * device.sector_size),
                    partition.type_code,
                    dos_type_name(partition.type_code)
                );
            }
        }
        DiskLabel::Gpt(table) => {
            println!();
            println!(
                "{:<16} {:>10} {:>10} {:>10} {:>8} Type",
                "Device", "Start", "End", "Sectors", "Size"
            );
            for (number, partition) in &table.partitions {
                let sectors = partition.last_lba - partition.first_lba + 1;
                println!(
                    "{:<16} {:>10} {:>10} {:>10} {:>8} {}",
                    partition_path(&device.path, *number),
                    partition.first_lba,
                    partition.last_lba,
                    sectors,
                    human_size(sectors * device.sector_size),
                    gpt_type_name(partition.type_guid)
                );
            }
        }
    }
}

fn disk_label_name(label: &DiskLabel) -> &'static str {
    match label {
        DiskLabel::None => "none",
        DiskLabel::Dos(_) => "dos",
        DiskLabel::Gpt(_) => "gpt",
    }
}

fn add_partition(label: &mut DiskLabel, device: &Device, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    match label {
        DiskLabel::None => Err(vec![AppletError::new(
            APPLET,
            "create a disklabel first with 'o' or 'g'",
        )]),
        DiskLabel::Dos(table) => add_dos_partition(table, device, reader),
        DiskLabel::Gpt(table) => add_gpt_partition(table, device, reader),
    }
}

fn add_dos_partition(table: &mut DosTable, device: &Device, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    if table.partitions.len() >= 4 {
        return Err(vec![AppletError::new(APPLET, "no free partition slots")]);
    }
    let default_slot = (1..=4)
        .find(|slot| !table.partitions.contains_key(slot))
        .unwrap();
    let slot = prompt_number(reader, "Partition number (1-4)", default_slot, 1, 4)?;
    if table.partitions.contains_key(&slot) {
        return Err(vec![AppletError::new(
            APPLET,
            format!("partition {slot} already exists"),
        )]);
    }
    let ranges = free_ranges(
        table.partitions.values().map(|partition| (partition.start_lba as u64, partition.end_lba() as u64)),
        dos_first_usable(device.total_sectors),
        device.total_sectors.saturating_sub(1),
    );
    let default_start = default_start_from_ranges(&ranges)?;
    let start = prompt_sector(reader, "First sector", default_start)?;
    let max_end = range_end_for_start(&ranges, start)?;
    let end = prompt_last_sector(reader, start, max_end, device.sector_size)?;
    let sector_count = end
        .checked_sub(start)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| vec![AppletError::new(APPLET, "partition size overflow")])?;
    if sector_count > u32::MAX as u64 {
        return Err(vec![AppletError::new(APPLET, "partition is too large for DOS")]);
    }
    table.partitions.insert(
        slot,
        DosPartition {
            bootable: false,
            type_code: 0x83,
            start_lba: start as u32,
            sector_count: sector_count as u32,
        },
    );
    Ok(())
}

fn add_gpt_partition(table: &mut GptTable, device: &Device, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    let default_slot = (1..=table.entry_count)
        .find(|slot| !table.partitions.contains_key(slot))
        .ok_or_else(|| vec![AppletError::new(APPLET, "no free partition slots")])?;
    let slot = prompt_number(reader, "Partition number", default_slot, 1, table.entry_count)?;
    if table.partitions.contains_key(&slot) {
        return Err(vec![AppletError::new(
            APPLET,
            format!("partition {slot} already exists"),
        )]);
    }
    let ranges = free_ranges(
        table.partitions.values().map(|partition| (partition.first_lba, partition.last_lba)),
        table.first_usable_lba,
        table.last_usable_lba,
    );
    let default_start = default_start_from_ranges(&ranges)?;
    let start = prompt_sector(reader, "First sector", default_start)?;
    let max_end = range_end_for_start(&ranges, start)?;
    let end = prompt_last_sector(reader, start, max_end, device.sector_size)?;
    table.partitions.insert(
        slot,
        GptPartition {
            type_guid: GUID_LINUX_FILESYSTEM,
            partition_guid: random_guid()?,
            first_lba: start,
            last_lba: end,
            name: String::new(),
        },
    );
    Ok(())
}

fn delete_partition(label: &mut DiskLabel, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    match label {
        DiskLabel::None => Err(vec![AppletError::new(APPLET, "no disklabel present")]),
        DiskLabel::Dos(table) => {
            let default = *table
                .partitions
                .keys()
                .next()
                .ok_or_else(|| vec![AppletError::new(APPLET, "no partitions defined")])?;
            let slot = prompt_number(reader, "Partition number (1-4)", default, 1, 4)?;
            table.partitions.remove(&slot);
            Ok(())
        }
        DiskLabel::Gpt(table) => {
            let default = *table
                .partitions
                .keys()
                .next()
                .ok_or_else(|| vec![AppletError::new(APPLET, "no partitions defined")])?;
            let slot = prompt_number(reader, "Partition number", default, 1, table.entry_count)?;
            table.partitions.remove(&slot);
            Ok(())
        }
    }
}

fn change_partition_type(label: &mut DiskLabel, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    match label {
        DiskLabel::None => Err(vec![AppletError::new(APPLET, "no disklabel present")]),
        DiskLabel::Dos(table) => {
            let default = *table
                .partitions
                .keys()
                .next()
                .ok_or_else(|| vec![AppletError::new(APPLET, "no partitions defined")])?;
            let slot = prompt_number(reader, "Partition number (1-4)", default, 1, 4)?;
            let value = reader.read_line("Hex code (e.g. 83 for Linux): ")?;
            let code = parse_dos_type(&value)?;
            let partition = table
                .partitions
                .get_mut(&slot)
                .ok_or_else(|| vec![AppletError::new(APPLET, format!("partition {slot} not found"))])?;
            partition.type_code = code;
            Ok(())
        }
        DiskLabel::Gpt(table) => {
            let default = *table
                .partitions
                .keys()
                .next()
                .ok_or_else(|| vec![AppletError::new(APPLET, "no partitions defined")])?;
            let slot = prompt_number(reader, "Partition number", default, 1, table.entry_count)?;
            let value = reader.read_line("Partition type (efi, linux, swap, bios, GUID): ")?;
            let guid = parse_gpt_type(&value)?;
            let partition = table
                .partitions
                .get_mut(&slot)
                .ok_or_else(|| vec![AppletError::new(APPLET, format!("partition {slot} not found"))])?;
            partition.type_guid = guid;
            Ok(())
        }
    }
}

fn toggle_bootable(label: &mut DiskLabel, reader: &mut CommandReader) -> Result<(), Vec<AppletError>> {
    match label {
        DiskLabel::Dos(table) => {
            let default = *table
                .partitions
                .keys()
                .next()
                .ok_or_else(|| vec![AppletError::new(APPLET, "no partitions defined")])?;
            let slot = prompt_number(reader, "Partition number (1-4)", default, 1, 4)?;
            let partition = table
                .partitions
                .get_mut(&slot)
                .ok_or_else(|| vec![AppletError::new(APPLET, format!("partition {slot} not found"))])?;
            partition.bootable = !partition.bootable;
            Ok(())
        }
        DiskLabel::Gpt(_) => Err(vec![AppletError::new(
            APPLET,
            "bootable flag is only supported for DOS labels",
        )]),
        DiskLabel::None => Err(vec![AppletError::new(APPLET, "no disklabel present")]),
    }
}

fn write_disk_label(device: &mut Device, label: &DiskLabel) -> Result<(), Vec<AppletError>> {
    match label {
        DiskLabel::None => Err(vec![AppletError::new(APPLET, "no disklabel to write")]),
        DiskLabel::Dos(table) => write_dos_label(device, table),
        DiskLabel::Gpt(table) => write_gpt_label(device, table),
    }?;
    device
        .file
        .sync_all()
        .map_err(|err| vec![AppletError::from_io(APPLET, "syncing", Some(&device.path), err)])?;
    if device.is_block
        && let Err(error) = reread_partition_table(device.file.as_raw_fd())
    {
        eprintln!("warning: {error}");
    }
    Ok(())
}

fn write_dos_label(device: &mut Device, table: &DosTable) -> Result<(), Vec<AppletError>> {
    let mut sector0 = vec![0_u8; device.sector_size as usize];
    for slot in 1..=4 {
        let offset = MBR_PARTITION_OFFSET + (slot as usize - 1) * 16;
        if let Some(partition) = table.partitions.get(&slot) {
            sector0[offset] = if partition.bootable { 0x80 } else { 0x00 };
            sector0[offset + 1..offset + 4].copy_from_slice(&[0xff, 0xff, 0xff]);
            sector0[offset + 4] = partition.type_code;
            sector0[offset + 5..offset + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
            sector0[offset + 8..offset + 12].copy_from_slice(&partition.start_lba.to_le_bytes());
            sector0[offset + 12..offset + 16]
                .copy_from_slice(&partition.sector_count.to_le_bytes());
        }
    }
    sector0[MBR_SIGNATURE_OFFSET] = 0x55;
    sector0[MBR_SIGNATURE_OFFSET + 1] = 0xaa;
    write_lba(device, 0, &sector0)?;
    clear_stale_gpt(device)?;
    Ok(())
}

fn write_gpt_label(device: &mut Device, table: &GptTable) -> Result<(), Vec<AppletError>> {
    let entry_sectors = gpt_entry_sectors(device.sector_size, table.entry_count, table.entry_size);
    let entry_bytes = table.entry_count as usize * table.entry_size as usize;
    let mut entries = vec![0_u8; entry_bytes];
    for (slot, partition) in &table.partitions {
        let offset = (*slot as usize - 1) * table.entry_size as usize;
        entries[offset..offset + 16].copy_from_slice(&partition.type_guid.to_gpt_bytes());
        entries[offset + 16..offset + 32].copy_from_slice(&partition.partition_guid.to_gpt_bytes());
        entries[offset + 32..offset + 40].copy_from_slice(&partition.first_lba.to_le_bytes());
        entries[offset + 40..offset + 48].copy_from_slice(&partition.last_lba.to_le_bytes());
        entries[offset + 56..offset + 128].copy_from_slice(&encode_gpt_name(&partition.name));
    }
    let entries_crc = crc32(&entries);
    let primary_entries_lba = 2;
    let backup_entries_lba = device.total_sectors - entry_sectors - 1;
    let primary_header = build_gpt_header(
        table,
        device,
        1,
        device.total_sectors - 1,
        primary_entries_lba,
        entries_crc,
    );
    let backup_header = build_gpt_header(
        table,
        device,
        device.total_sectors - 1,
        1,
        backup_entries_lba,
        entries_crc,
    );

    write_lba(device, 0, &protective_mbr(device.total_sectors, device.sector_size))?;

    let mut primary_entries = vec![0_u8; (entry_sectors * device.sector_size) as usize];
    primary_entries[..entries.len()].copy_from_slice(&entries);
    write_lba(device, primary_entries_lba, &primary_entries)?;
    write_lba(device, 1, &primary_header)?;

    let mut backup_entries = vec![0_u8; (entry_sectors * device.sector_size) as usize];
    backup_entries[..entries.len()].copy_from_slice(&entries);
    write_lba(device, backup_entries_lba, &backup_entries)?;
    write_lba(device, device.total_sectors - 1, &backup_header)?;
    Ok(())
}

fn clear_stale_gpt(device: &mut Device) -> Result<(), Vec<AppletError>> {
    let wipe_sectors = 33_u64.min(device.total_sectors.saturating_sub(1));
    if wipe_sectors == 0 {
        return Ok(());
    }
    let zeros = vec![0_u8; (wipe_sectors * device.sector_size) as usize];
    write_lba(device, 1, &zeros)?;
    if device.total_sectors > wipe_sectors {
        let tail_start = device.total_sectors.saturating_sub(wipe_sectors);
        write_lba(device, tail_start, &zeros)?;
    }
    Ok(())
}

fn protective_mbr(total_sectors: u64, sector_size: u64) -> Vec<u8> {
    let mut sector0 = vec![0_u8; sector_size as usize];
    let size = total_sectors.saturating_sub(1).min(u32::MAX as u64) as u32;
    sector0[MBR_PARTITION_OFFSET + 1..MBR_PARTITION_OFFSET + 4].copy_from_slice(&[0x00, 0x02, 0x00]);
    sector0[MBR_PARTITION_OFFSET + 4] = 0xee;
    sector0[MBR_PARTITION_OFFSET + 5..MBR_PARTITION_OFFSET + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
    sector0[MBR_PARTITION_OFFSET + 8..MBR_PARTITION_OFFSET + 12].copy_from_slice(&1_u32.to_le_bytes());
    sector0[MBR_PARTITION_OFFSET + 12..MBR_PARTITION_OFFSET + 16].copy_from_slice(&size.to_le_bytes());
    sector0[MBR_SIGNATURE_OFFSET] = 0x55;
    sector0[MBR_SIGNATURE_OFFSET + 1] = 0xaa;
    sector0
}

fn build_gpt_header(
    table: &GptTable,
    device: &Device,
    current_lba: u64,
    backup_lba: u64,
    entries_lba: u64,
    entries_crc: u32,
) -> Vec<u8> {
    let mut header = vec![0_u8; device.sector_size as usize];
    header[0..8].copy_from_slice(b"EFI PART");
    header[8..12].copy_from_slice(&0x0001_0000_u32.to_le_bytes());
    header[12..16].copy_from_slice(&GPT_HEADER_SIZE.to_le_bytes());
    header[24..32].copy_from_slice(&current_lba.to_le_bytes());
    header[32..40].copy_from_slice(&backup_lba.to_le_bytes());
    header[40..48].copy_from_slice(&table.first_usable_lba.to_le_bytes());
    header[48..56].copy_from_slice(&table.last_usable_lba.to_le_bytes());
    header[56..72].copy_from_slice(&table.disk_guid.to_gpt_bytes());
    header[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    header[80..84].copy_from_slice(&table.entry_count.to_le_bytes());
    header[84..88].copy_from_slice(&table.entry_size.to_le_bytes());
    header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let crc = crc32(&header[..GPT_HEADER_SIZE as usize]);
    header[16..20].copy_from_slice(&crc.to_le_bytes());
    header
}

fn new_gpt_table(total_sectors: u64, sector_size: u64) -> Result<GptTable, Vec<AppletError>> {
    let entry_sectors = gpt_entry_sectors(sector_size, GPT_ENTRY_COUNT, GPT_ENTRY_SIZE);
    let first_usable_lba = 2 + entry_sectors;
    let last_usable_lba = total_sectors
        .checked_sub(entry_sectors + 2)
        .ok_or_else(|| vec![AppletError::new(APPLET, "disk is too small for GPT")])?;
    if last_usable_lba <= first_usable_lba {
        return Err(vec![AppletError::new(APPLET, "disk is too small for GPT")]);
    }
    Ok(GptTable {
        disk_guid: random_guid()?,
        partitions: BTreeMap::new(),
        entry_count: GPT_ENTRY_COUNT,
        entry_size: GPT_ENTRY_SIZE,
        first_usable_lba,
        last_usable_lba,
    })
}

fn gpt_entry_sectors(sector_size: u64, entry_count: u32, entry_size: u32) -> u64 {
    div_ceil(entry_count as u64 * entry_size as u64, sector_size)
}

fn div_ceil(left: u64, right: u64) -> u64 {
    left / right + u64::from(!left.is_multiple_of(right))
}

fn prompt_number(
    reader: &mut CommandReader,
    prompt: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32, Vec<AppletError>> {
    let line = reader.read_line(&format!("{prompt} (default {default}): "))?;
    let value = if line.is_empty() {
        default
    } else {
        line.parse::<u32>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid number '{line}'"),
            )]
        })?
    };
    if value < min || value > max {
        return Err(vec![AppletError::new(
            APPLET,
            format!("number '{value}' is out of range"),
        )]);
    }
    Ok(value)
}

fn prompt_sector(reader: &mut CommandReader, prompt: &str, default: u64) -> Result<u64, Vec<AppletError>> {
    let line = reader.read_line(&format!("{prompt} (default {default}): "))?;
    if line.is_empty() {
        return Ok(default);
    }
    line.parse::<u64>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid sector '{line}'"))])
}

fn prompt_last_sector(
    reader: &mut CommandReader,
    start: u64,
    default_end: u64,
    sector_size: u64,
) -> Result<u64, Vec<AppletError>> {
    let line = reader.read_line(&format!(
        "Last sector, +/-sectors or +/-size{{K,M,G,T}} (default {default_end}): "
    ))?;
    let end = if line.is_empty() {
        default_end
    } else if let Some(size) = line.strip_prefix('+') {
        let sectors = parse_size_expression(size, sector_size)?;
        start
            .checked_add(sectors)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| vec![AppletError::new(APPLET, "partition end overflow")])?
    } else {
        line.parse::<u64>()
            .map_err(|_| vec![AppletError::new(APPLET, format!("invalid sector '{line}'"))])?
    };
    if end < start || end > default_end {
        return Err(vec![AppletError::new(
            APPLET,
            "partition does not fit in the available space",
        )]);
    }
    Ok(end)
}

fn parse_size_expression(value: &str, sector_size: u64) -> Result<u64, Vec<AppletError>> {
    let split_at = value
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(value.len());
    let (digits, suffix) = value.split_at(split_at);
    let number = digits.parse::<u64>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid size '{value}'"),
        )]
    })?;
    let bytes = match suffix.to_ascii_lowercase().as_str() {
        "" => return Ok(number),
        "k" => number.saturating_mul(1024),
        "m" => number.saturating_mul(1024 * 1024),
        "g" => number.saturating_mul(1024 * 1024 * 1024),
        "t" => number.saturating_mul(1024_u64.pow(4)),
        _ => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid size '{value}'"),
            )]);
        }
    };
    Ok(div_ceil(bytes, sector_size))
}

fn free_ranges(
    used: impl Iterator<Item = (u64, u64)>,
    min_start: u64,
    max_end: u64,
) -> Vec<(u64, u64)> {
    let mut ranges = used.collect::<Vec<_>>();
    ranges.sort_unstable();
    let mut free = Vec::new();
    let mut cursor = min_start;
    for (start, end) in ranges {
        if end < cursor {
            continue;
        }
        if start > cursor {
            free.push((cursor, start - 1));
        }
        cursor = end.saturating_add(1);
        if cursor > max_end {
            break;
        }
    }
    if cursor <= max_end {
        free.push((cursor, max_end));
    }
    free
}

fn default_start_from_ranges(ranges: &[(u64, u64)]) -> Result<u64, Vec<AppletError>> {
    for (start, end) in ranges {
        let aligned = align_up(*start, ALIGNMENT_SECTORS);
        if aligned <= *end {
            return Ok(aligned);
        }
        if *start <= *end {
            return Ok(*start);
        }
    }
    Err(vec![AppletError::new(APPLET, "no free space available")])
}

fn range_end_for_start(ranges: &[(u64, u64)], start: u64) -> Result<u64, Vec<AppletError>> {
    ranges
        .iter()
        .find(|(range_start, range_end)| start >= *range_start && start <= *range_end)
        .map(|(_, end)| *end)
        .ok_or_else(|| vec![AppletError::new(APPLET, "requested start sector is not free")])
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        return value;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + (alignment - remainder)
    }
}

fn dos_first_usable(total_sectors: u64) -> u64 {
    if total_sectors > ALIGNMENT_SECTORS {
        ALIGNMENT_SECTORS
    } else {
        1
    }
}

fn parse_dos_type(value: &str) -> Result<u8, Vec<AppletError>> {
    let trimmed = value.trim().trim_start_matches("0x");
    u8::from_str_radix(trimmed, 16).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid DOS partition type '{value}'"),
        )]
    })
}

fn parse_gpt_type(value: &str) -> Result<Guid, Vec<AppletError>> {
    let lowered = value.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "" | "linux" | "linuxfs" | "83" => Ok(GUID_LINUX_FILESYSTEM),
        "efi" | "esp" | "ef" => Ok(GUID_EFI_SYSTEM),
        "swap" | "82" => Ok(GUID_LINUX_SWAP),
        "bios" | "bios_grub" => Ok(GUID_BIOS_BOOT),
        _ => Guid::parse(&lowered).ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("invalid GPT partition type '{value}'"),
            )]
        }),
    }
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    const TIB: u64 = 1024_u64.pow(4);
    if bytes >= TIB && bytes.is_multiple_of(TIB) {
        format!("{}T", bytes / TIB)
    } else if bytes >= GIB && bytes.is_multiple_of(GIB) {
        format!("{}G", bytes / GIB)
    } else if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{}M", bytes / MIB)
    } else if bytes >= KIB && bytes.is_multiple_of(KIB) {
        format!("{}K", bytes / KIB)
    } else {
        format!("{bytes}B")
    }
}

fn partition_path(base: &str, number: u32) -> String {
    if base
        .chars()
        .last()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        format!("{base}p{number}")
    } else {
        format!("{base}{number}")
    }
}

fn dos_type_name(type_code: u8) -> &'static str {
    match type_code {
        0x82 => "Linux swap",
        0x83 => "Linux",
        0x07 => "HPFS/NTFS/exFAT",
        0x0b => "W95 FAT32",
        0x0c => "W95 FAT32 (LBA)",
        0xef => "EFI (FAT-12/16/32)",
        0xee => "GPT protective",
        _ => "Unknown",
    }
}

fn gpt_type_name(type_guid: Guid) -> &'static str {
    match type_guid {
        GUID_EFI_SYSTEM => "EFI System",
        GUID_LINUX_FILESYSTEM => "Linux filesystem",
        GUID_LINUX_SWAP => "Linux swap",
        GUID_BIOS_BOOT => "BIOS boot",
        _ => "Unknown",
    }
}

fn print_help() {
    eprintln!("m   print this menu");
    eprintln!("p   print the partition table");
    eprintln!("o   create a new empty DOS partition table");
    eprintln!("g   create a new empty GPT partition table");
    eprintln!("n   add a new partition");
    eprintln!("d   delete a partition");
    eprintln!("t   change a partition type");
    eprintln!("a   toggle a bootable flag (DOS only)");
    eprintln!("w   write table to disk and exit");
    eprintln!("q   quit without saving changes");
}

fn block_sector_size(fd: libc::c_int) -> io::Result<u64> {
    let mut value = 0_i32;
    let rc = unsafe { libc::ioctl(fd, BLKSSZGET as _, &mut value) };
    if rc == 0 {
        Ok(value as u64)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn block_device_size(fd: libc::c_int) -> Option<u64> {
    let mut value = 0_u64;
    let rc = unsafe { libc::ioctl(fd, BLKGETSIZE64 as _, &mut value) };
    (rc == 0).then_some(value)
}

fn reread_partition_table(fd: libc::c_int) -> Result<(), String> {
    let rc = unsafe { libc::ioctl(fd, BLKRRPART as _, 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "rereading partition table failed: {}",
            io::Error::last_os_error()
        ))
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & 0xedb8_8320;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

fn decode_gpt_name(bytes: &[u8]) -> String {
    let mut code_units = Vec::new();
    for chunk in bytes.chunks_exact(2) {
        let unit = u16::from_le_bytes([chunk[0], chunk[1]]);
        if unit == 0 {
            break;
        }
        code_units.push(unit);
    }
    String::from_utf16_lossy(&code_units)
}

fn encode_gpt_name(name: &str) -> [u8; 72] {
    let mut bytes = [0_u8; 72];
    for (index, unit) in name.encode_utf16().take(36).enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn random_guid() -> Result<Guid, Vec<AppletError>> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/dev/urandom"), err)])?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Guid(bytes))
}

impl Guid {
    fn from_gpt_bytes(bytes: &[u8]) -> Self {
        let mut canonical = [0_u8; 16];
        canonical[0..4].copy_from_slice(&[bytes[3], bytes[2], bytes[1], bytes[0]]);
        canonical[4..6].copy_from_slice(&[bytes[5], bytes[4]]);
        canonical[6..8].copy_from_slice(&[bytes[7], bytes[6]]);
        canonical[8..16].copy_from_slice(&bytes[8..16]);
        Self(canonical)
    }

    fn to_gpt_bytes(self) -> [u8; 16] {
        let mut bytes = [0_u8; 16];
        bytes[0..4].copy_from_slice(&[self.0[3], self.0[2], self.0[1], self.0[0]]);
        bytes[4..6].copy_from_slice(&[self.0[5], self.0[4]]);
        bytes[6..8].copy_from_slice(&[self.0[7], self.0[6]]);
        bytes[8..16].copy_from_slice(&self.0[8..16]);
        bytes
    }

    fn parse(value: &str) -> Option<Self> {
        let hex = value.replace('-', "");
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0_u8; 16];
        for index in 0..16 {
            bytes[index] = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for Guid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3],
            self.0[4],
            self.0[5],
            self.0[6],
            self.0[7],
            self.0[8],
            self.0[9],
            self.0[10],
            self.0[11],
            self.0[12],
            self.0[13],
            self.0[14],
            self.0[15],
        )
    }
}

impl DosPartition {
    fn end_lba(&self) -> u32 {
        self.start_lba + self.sector_count.saturating_sub(1)
    }
}

struct CommandReader {
    stdin: io::Stdin,
    is_tty: bool,
}

impl CommandReader {
    fn new() -> Self {
        Self {
            stdin: io::stdin(),
            is_tty: unsafe { libc::isatty(libc::STDIN_FILENO) == 1 },
        }
    }

    fn read_line(&mut self, prompt: &str) -> Result<String, Vec<AppletError>> {
        if self.is_tty {
            eprint!("{prompt}");
            io::stderr()
                .flush()
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        }
        let mut line = String::new();
        let read = self
            .stdin
            .lock()
            .read_line(&mut line)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        if read == 0 {
            return Err(vec![AppletError::new(APPLET, "unexpected end of input")]);
        }
        Ok(line.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DosPartition, DosTable, GUID_EFI_SYSTEM, GUID_LINUX_FILESYSTEM, GptPartition, GptTable,
        Guid, crc32, decode_gpt_name, encode_gpt_name, free_ranges, gpt_entry_sectors,
        parse_dos_table, parse_size_expression, partition_path, protective_mbr,
    };
    use std::collections::BTreeMap;

    #[test]
    fn parses_mbr_partition_entries() {
        let mut sector0 = vec![0_u8; 512];
        sector0[446] = 0x80;
        sector0[450] = 0x83;
        sector0[454..458].copy_from_slice(&2048_u32.to_le_bytes());
        sector0[458..462].copy_from_slice(&32768_u32.to_le_bytes());
        sector0[510] = 0x55;
        sector0[511] = 0xaa;
        let table = parse_dos_table(&sector0);
        let partition = table.partitions.get(&1).unwrap();
        assert!(partition.bootable);
        assert_eq!(partition.start_lba, 2048);
        assert_eq!(partition.sector_count, 32768);
    }

    #[test]
    fn parses_size_expressions() {
        assert_eq!(parse_size_expression("16M", 512).unwrap(), 32768);
        assert_eq!(parse_size_expression("2048", 512).unwrap(), 2048);
    }

    #[test]
    fn computes_free_ranges() {
        let free = free_ranges([(2048, 4095), (8192, 12287)].into_iter(), 2048, 16383);
        assert_eq!(free, vec![(4096, 8191), (12288, 16383)]);
    }

    #[test]
    fn round_trips_gpt_names() {
        let bytes = encode_gpt_name("EFI system");
        assert_eq!(decode_gpt_name(&bytes), "EFI system");
    }

    #[test]
    fn formats_partition_paths() {
        assert_eq!(partition_path("/tmp/disk.img", 1), "/tmp/disk.img1");
        assert_eq!(partition_path("/dev/nvme0n1", 2), "/dev/nvme0n1p2");
    }

    #[test]
    fn protective_mbr_marks_gpt_partition() {
        let sector0 = protective_mbr(131072, 512);
        assert_eq!(sector0[450], 0xee);
        assert_eq!(sector0[510], 0x55);
        assert_eq!(sector0[511], 0xaa);
    }

    #[test]
    fn guid_gpt_byte_order_round_trip() {
        let guid = Guid::parse("c12a7328-f81f-11d2-ba4b-00a0c93ec93b").unwrap();
        assert_eq!(Guid::from_gpt_bytes(&guid.to_gpt_bytes()), guid);
    }

    #[test]
    fn crc32_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn gpt_entry_sectors_match_default_layout() {
        assert_eq!(gpt_entry_sectors(512, 128, 128), 32);
    }

    #[test]
    fn gpt_partition_types_are_stable() {
        let mut partitions = BTreeMap::new();
        partitions.insert(
            1,
            GptPartition {
                type_guid: GUID_EFI_SYSTEM,
                partition_guid: GUID_LINUX_FILESYSTEM,
                first_lba: 2048,
                last_lba: 4095,
                name: String::new(),
            },
        );
        let table = GptTable {
            disk_guid: GUID_LINUX_FILESYSTEM,
            partitions,
            entry_count: 128,
            entry_size: 128,
            first_usable_lba: 34,
            last_usable_lba: 131038,
        };
        assert_eq!(table.partitions.get(&1).unwrap().type_guid, GUID_EFI_SYSTEM);
    }

    #[test]
    fn dos_partition_end_lba_is_inclusive() {
        let partition = DosPartition {
            bootable: false,
            type_code: 0x83,
            start_lba: 2048,
            sector_count: 32768,
        };
        assert_eq!(partition.end_lba(), 34815);
    }

    #[test]
    fn empty_dos_table_starts_empty() {
        assert_eq!(DosTable::default().partitions.len(), 0);
    }
}
