use std::path::Path;

#[cfg(target_os = "linux")]
use std::fs;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessInfo {
    pub(crate) pid: i32,
    pub(crate) tty: String,
    pub(crate) cpu_time_ns: u64,
    pub(crate) name: String,
    pub(crate) command: String,
}

impl ProcessInfo {
    pub(crate) fn matches_pattern(&self, pattern: &str) -> bool {
        self.name.contains(pattern)
            || self
                .command
                .split_whitespace()
                .next()
                .is_some_and(|part| basename(part).contains(pattern))
    }

    pub(crate) fn matches_exact_name(&self, target: &str) -> bool {
        self.name == target
            || self
                .command
                .split_whitespace()
                .next()
                .is_some_and(|part| basename(part) == target)
    }
}

fn basename(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
}

#[cfg(target_os = "linux")]
pub(crate) fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    let mut processes = Vec::new();

    for entry in fs::read_dir("/proc").map_err(|_| String::from("listing processes failed"))? {
        let entry = entry.map_err(|_| String::from("listing processes failed"))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = file_name.parse::<i32>() else {
            continue;
        };
        if let Some(process) = process_info_linux(pid) {
            processes.push(process);
        }
    }

    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    Err(String::from("unsupported on this platform"))
}

#[cfg(target_os = "linux")]
fn process_info_linux(pid: i32) -> Option<ProcessInfo> {
    let stat = parse_linux_stat(pid)?;
    let command = command_line_for_pid_linux(pid).unwrap_or_else(|| stat.name.clone());

    Some(ProcessInfo {
        pid,
        tty: tty_for_pid_linux(pid),
        cpu_time_ns: stat.cpu_time_ns,
        name: stat.name,
        command,
    })
}

#[cfg(target_os = "linux")]
struct LinuxStat {
    name: String,
    cpu_time_ns: u64,
}

#[cfg(target_os = "linux")]
fn parse_linux_stat(pid: i32) -> Option<LinuxStat> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_linux_stat_text(&stat)
}

#[cfg(target_os = "linux")]
fn parse_linux_stat_text(stat: &str) -> Option<LinuxStat> {
    let open = stat.find('(')?;
    let close = stat.rfind(") ")?;
    if close <= open {
        return None;
    }

    let name = stat[open + 1..close].to_string();
    let rest = &stat[close + 2..];
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    let ticks = utime.saturating_add(stime);
    let ticks_per_second = clock_ticks_per_second().ok()?;
    let cpu_time_ns = ticks
        .saturating_mul(1_000_000_000)
        .checked_div(ticks_per_second)
        .unwrap_or(0);

    Some(LinuxStat { name, cpu_time_ns })
}

#[cfg(target_os = "linux")]
fn clock_ticks_per_second() -> Result<u64, String> {
    // SAFETY: `sysconf` reads process-global configuration and has no side effects.
    let value = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if value <= 0 {
        Err(String::from("reading clock tick size failed"))
    } else {
        Ok(value as u64)
    }
}

#[cfg(target_os = "linux")]
fn command_line_for_pid_linux(pid: i32) -> Option<String> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>();
    if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    }
}

#[cfg(target_os = "linux")]
fn tty_for_pid_linux(pid: i32) -> String {
    let path = format!("/proc/{pid}/fd/0");
    match fs::read_link(path) {
        Ok(target) => {
            let target = target.to_string_lossy();
            if let Some(name) = target.strip_prefix("/dev/") {
                name.to_string()
            } else {
                String::from("??")
            }
        }
        Err(_) => String::from("??"),
    }
}

#[cfg(test)]
mod tests {
    use super::ProcessInfo;
    #[cfg(target_os = "linux")]
    use super::{list_processes, parse_linux_stat_text};

    #[cfg(target_os = "linux")]
    #[test]
    fn process_list_contains_current_process() {
        let pid = std::process::id() as i32;
        let processes = list_processes().expect("list processes");
        assert!(processes.iter().any(|process| process.pid == pid));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_stat_format() {
        let stat = parse_linux_stat_text("1234 (seed worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14")
            .expect("parse stat");
        assert_eq!(stat.name, "seed worker");
        assert!(stat.cpu_time_ns > 0);
    }

    #[test]
    fn pattern_matching_uses_name_and_argv0_only() {
        let process = ProcessInfo {
            pid: 1,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("match-proc"),
            command: String::from("/bin/sh ./wrapper-target --flag"),
        };
        assert!(process.matches_pattern("match-proc"));
        assert!(!process.matches_pattern("wrapper-target"));
        assert!(!process.matches_exact_name("wrapper-target"));
    }
}
