use std::path::Path;

#[cfg(target_os = "macos")]
use std::ffi::CStr;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "macos")]
use std::mem::MaybeUninit;

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
        self.name.contains(pattern) || self.command.contains(pattern)
    }

    pub(crate) fn matches_exact_name(&self, target: &str) -> bool {
        self.name == target
            || self
                .command
                .split_whitespace()
                .any(|part| basename(part) == target)
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

#[cfg(target_os = "macos")]
pub(crate) fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    // SAFETY: null buffer with size 0 asks libproc for the current pid count.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(String::from("listing processes failed"));
    }

    let mut pids = vec![0 as libc::pid_t; count as usize];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
    // SAFETY: `pids` points to `bytes` writable bytes for libproc to fill.
    let actual = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    if actual <= 0 {
        return Err(String::from("listing processes failed"));
    }
    pids.truncate(actual as usize);

    let mut processes = Vec::new();
    for pid in pids {
        if pid <= 0 {
            continue;
        }
        if let Some(process) = process_info(pid) {
            processes.push(process);
        }
    }
    processes.sort_by_key(|process| process.pid);
    Ok(processes)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

#[cfg(target_os = "macos")]
fn process_info(pid: libc::pid_t) -> Option<ProcessInfo> {
    let mut info = MaybeUninit::<libc::proc_taskallinfo>::uninit();
    let expected = std::mem::size_of::<libc::proc_taskallinfo>() as libc::c_int;
    // SAFETY: `info` points to a writable buffer of `expected` bytes.
    let actual = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKALLINFO,
            0,
            info.as_mut_ptr().cast(),
            expected,
        )
    };
    if actual != expected {
        return None;
    }

    // SAFETY: libproc initialized `info` because the returned size matched.
    let info = unsafe { info.assume_init() };
    let name = c_char_array_to_string(&info.pbsd.pbi_name)
        .filter(|value| !value.is_empty())
        .or_else(|| c_char_array_to_string(&info.pbsd.pbi_comm))
        .unwrap_or_else(|| pid.to_string());
    let command = command_line_for_pid(pid)
        .or_else(|| path_for_pid(pid))
        .unwrap_or_else(|| name.clone());

    Some(ProcessInfo {
        pid,
        tty: tty_name(info.pbsd.e_tdev),
        cpu_time_ns: info.ptinfo.pti_total_user + info.ptinfo.pti_total_system,
        name,
        command,
    })
}

#[cfg(target_os = "macos")]
fn path_for_pid(pid: libc::pid_t) -> Option<String> {
    let mut buffer = [0_i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buffer` points to writable storage of the advertised size.
    let actual = unsafe {
        libc::proc_pidpath(
            pid,
            buffer.as_mut_ptr().cast(),
            libc::PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if actual <= 0 {
        return None;
    }
    // SAFETY: `proc_pidpath` writes a NUL-terminated string on success.
    Some(
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(target_os = "macos")]
fn command_line_for_pid(pid: libc::pid_t) -> Option<String> {
    let argmax = argmax().ok()?;
    let mut buffer = vec![0_u8; argmax];
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = buffer.len();

    // SAFETY: `mib` contains valid sysctl selectors, `buffer` points to
    // writable memory, and `size` describes that buffer.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size < std::mem::size_of::<i32>() {
        return None;
    }

    buffer.truncate(size);
    let argc = i32::from_ne_bytes(buffer[..4].try_into().ok()?) as usize;
    let mut index = 4;

    while index < buffer.len() && buffer[index] != 0 {
        index += 1;
    }
    while index < buffer.len() && buffer[index] == 0 {
        index += 1;
    }

    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        if index >= buffer.len() {
            break;
        }
        let start = index;
        while index < buffer.len() && buffer[index] != 0 {
            index += 1;
        }
        if start == index {
            break;
        }
        args.push(String::from_utf8_lossy(&buffer[start..index]).into_owned());
        while index < buffer.len() && buffer[index] == 0 {
            index += 1;
        }
    }

    if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    }
}

#[cfg(target_os = "macos")]
fn argmax() -> Result<usize, String> {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value = 0_i32;
    let mut size = std::mem::size_of::<i32>();

    // SAFETY: `mib` is a valid sysctl selector and `value` points to writable
    // storage of `size` bytes.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            (&mut value as *mut i32).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || value <= 0 {
        Err(String::from("reading process arguments failed"))
    } else {
        Ok(value as usize)
    }
}

#[cfg(target_os = "macos")]
fn tty_name(device: u32) -> String {
    if device == 0 {
        return String::from("??");
    }
    // SAFETY: `device` is a raw tty device number obtained from libproc.
    let name = unsafe { libc::devname(device as libc::dev_t, libc::S_IFCHR) };
    if name.is_null() {
        String::from("??")
    } else {
        // SAFETY: `devname` returns a valid NUL-terminated static string.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(target_os = "macos")]
fn c_char_array_to_string(bytes: &[libc::c_char]) -> Option<String> {
    let len = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    if len == 0 {
        return None;
    }
    let raw = bytes[..len]
        .iter()
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    Some(String::from_utf8_lossy(&raw).into_owned())
}

#[cfg(test)]
mod tests {
    use super::list_processes;

    #[cfg(target_os = "linux")]
    use super::parse_linux_stat_text;

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
}
