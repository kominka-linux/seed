#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
#[cfg(target_os = "linux")]
use crate::common::io::stdout;

const APPLET: &str = "rfkill";
#[cfg(target_os = "linux")]
const SYS_CLASS_RFKILL: &str = "/sys/class/rfkill";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    List,
    Block,
    Unblock,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Target {
    All,
    Index(u32),
    Type(&'static str),
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct RfkillEntry {
    index: u32,
    name: String,
    kind: String,
    soft: bool,
    hard: bool,
    soft_path: PathBuf,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (command, target) = parse_args(args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(command, target);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = (command, target);

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

fn parse_args(args: &[String]) -> Result<(Command, Option<Target>), Vec<AppletError>> {
    let Some(command) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };

    let command = match command.as_str() {
        "list" => Command::List,
        "block" => Command::Block,
        "unblock" => Command::Unblock,
        _ => return Err(vec![AppletError::new(APPLET, "missing operand")]),
    };

    let target = match args.get(1) {
        None => None,
        Some(value) => Some(parse_target(value)?),
    };

    if args.len() > 2 {
        return Err(vec![AppletError::new(APPLET, "extra operand")]);
    }

    if matches!(command, Command::Block | Command::Unblock) && target.is_none() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((command, target))
}

fn parse_target(value: &str) -> Result<Target, Vec<AppletError>> {
    match value {
        "all" => Ok(Target::All),
        "wlan" | "wifi" => Ok(Target::Type("wlan")),
        "bluetooth" => Ok(Target::Type("bluetooth")),
        "uwb" => Ok(Target::Type("uwb")),
        "wimax" => Ok(Target::Type("wimax")),
        "wwan" => Ok(Target::Type("wwan")),
        "gps" => Ok(Target::Type("gps")),
        "fm" => Ok(Target::Type("fm")),
        _ => value
            .parse::<u32>()
            .map(Target::Index)
            .map_err(|_| vec![AppletError::new(APPLET, format!("invalid number '{value}'"))]),
    }
}

#[cfg(target_os = "linux")]
fn run_linux(command: Command, target: Option<Target>) -> AppletResult {
    let entries = read_rfkill_entries()?;
    match command {
        Command::List => list_entries(&entries, target.unwrap_or(Target::All)),
        Command::Block => set_blocked(&entries, target.expect("validated"), true),
        Command::Unblock => set_blocked(&entries, target.expect("validated"), false),
    }
}

#[cfg(target_os = "linux")]
fn read_rfkill_entries() -> Result<Vec<RfkillEntry>, Vec<AppletError>> {
    let directory = Path::new(SYS_CLASS_RFKILL);
    if !directory.exists() {
        return Err(vec![AppletError::from_io(
            APPLET,
            "opening",
            Some("/dev/rfkill"),
            std::io::Error::from(std::io::ErrorKind::NotFound),
        )]);
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(SYS_CLASS_RFKILL), err)])?
    {
        let entry =
            entry.map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(SYS_CLASS_RFKILL), err)])?;
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(index_text) = name.strip_prefix("rfkill") else {
            continue;
        };
        let Ok(index) = index_text.parse::<u32>() else {
            continue;
        };
        entries.push(RfkillEntry {
            index,
            name: read_trimmed(path.join("name"))?,
            kind: normalize_type(&read_trimmed(path.join("type"))?),
            soft: read_trimmed(path.join("soft"))? == "1",
            hard: read_trimmed(path.join("hard"))? == "1",
            soft_path: path.join("soft"),
        });
    }
    entries.sort_by_key(|entry| entry.index);
    Ok(entries)
}

#[cfg(target_os = "linux")]
fn read_trimmed(path: PathBuf) -> Result<String, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    fs::read_to_string(&path)
        .map(|value| value.trim().to_string())
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])
}

#[cfg(target_os = "linux")]
fn normalize_type(kind: &str) -> String {
    if kind == "wlan" {
        String::from("wlan")
    } else {
        kind.to_string()
    }
}

#[cfg(target_os = "linux")]
fn matches_target(entry: &RfkillEntry, target: Target) -> bool {
    match target {
        Target::All => true,
        Target::Index(index) => entry.index == index,
        Target::Type(kind) => entry.kind == kind,
    }
}

#[cfg(target_os = "linux")]
fn list_entries(entries: &[RfkillEntry], target: Target) -> AppletResult {
    let mut out = stdout();
    for entry in entries.iter().filter(|entry| matches_target(entry, target)) {
        writeln!(out, "{}: {}: {}", entry.index, entry.name, display_type(&entry.kind))
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        writeln!(
            out,
            "\tSoft blocked: {}",
            if entry.soft { "yes" } else { "no" }
        )
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        writeln!(
            out,
            "\tHard blocked: {}",
            if entry.hard { "yes" } else { "no" }
        )
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    }
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn display_type(kind: &str) -> &'static str {
    match kind {
        "wlan" => "Wireless LAN",
        "bluetooth" => "Bluetooth",
        "uwb" => "Ultra-Wideband",
        "wimax" => "WiMAX",
        "wwan" => "Wireless WAN",
        "gps" => "GPS",
        "fm" => "FM",
        _ => "Unknown",
    }
}

#[cfg(target_os = "linux")]
fn set_blocked(entries: &[RfkillEntry], target: Target, blocked: bool) -> AppletResult {
    let value = if blocked { "1" } else { "0" };
    for entry in entries.iter().filter(|entry| matches_target(entry, target)) {
        let path = entry.soft_path.to_string_lossy().into_owned();
        fs::write(&entry.soft_path, value)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&path), err)])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Command, Target, parse_args, parse_target};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_list_with_index() {
        assert_eq!(
            parse_args(&args(&["list", "4"])).expect("parse rfkill"),
            (Command::List, Some(Target::Index(4)))
        );
    }

    #[test]
    fn parses_block_with_type() {
        assert_eq!(
            parse_args(&args(&["block", "bluetooth"])).expect("parse rfkill"),
            (Command::Block, Some(Target::Type("bluetooth")))
        );
    }

    #[test]
    fn rejects_invalid_target() {
        assert!(parse_target("extra").is_err());
    }
}
