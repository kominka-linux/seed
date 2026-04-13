use std::ffi::CStr;
use std::mem::MaybeUninit;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProcessInfo {
    pub(crate) pid: i32,
    pub(crate) tty: String,
    pub(crate) cpu_time_ns: u64,
    pub(crate) name: String,
    pub(crate) command: String,
}

#[cfg(target_os = "macos")]
pub(crate) fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Err(String::from("listing processes failed"));
    }

    let mut pids = vec![0 as libc::pid_t; count as usize];
    let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
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

#[cfg(not(target_os = "macos"))]
pub(crate) fn list_processes() -> Result<Vec<ProcessInfo>, String> {
    Err(String::from("unsupported on this platform"))
}

#[cfg(target_os = "macos")]
fn process_info(pid: libc::pid_t) -> Option<ProcessInfo> {
    let mut info = MaybeUninit::<libc::proc_taskallinfo>::uninit();
    let expected = std::mem::size_of::<libc::proc_taskallinfo>() as libc::c_int;
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

    let info = unsafe { info.assume_init() };
    let name = c_char_array_to_string(&info.pbsd.pbi_name)
        .filter(|value| !value.is_empty())
        .or_else(|| c_char_array_to_string(&info.pbsd.pbi_comm))
        .unwrap_or_else(|| pid.to_string());
    let command = command_for_pid(pid).unwrap_or_else(|| name.clone());

    Some(ProcessInfo {
        pid,
        tty: tty_name(info.pbsd.e_tdev),
        cpu_time_ns: info.ptinfo.pti_total_user + info.ptinfo.pti_total_system,
        name,
        command,
    })
}

#[cfg(target_os = "macos")]
fn command_for_pid(pid: libc::pid_t) -> Option<String> {
    let mut buffer = [0_i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
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
    Some(
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(target_os = "macos")]
fn tty_name(device: u32) -> String {
    if device == 0 {
        return String::from("??");
    }
    let name = unsafe { libc::devname(device as libc::dev_t, libc::S_IFCHR) };
    if name.is_null() {
        String::from("??")
    } else {
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

    #[test]
    fn process_list_contains_current_process() {
        let pid = std::process::id() as i32;
        let processes = list_processes().expect("list processes");
        assert!(processes.iter().any(|process| process.pid == pid));
    }
}
