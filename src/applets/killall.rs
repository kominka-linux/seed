use crate::common::applet::finish_code;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::process::list_processes;

const APPLET: &str = "killall";
const LINUX_COMM_LIMIT: usize = 15;

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args).map(match_exit_code))
}

fn run(args: &[std::ffi::OsString]) -> Result<bool, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let target = parse_args(&args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let self_pid = std::process::id() as i32;
    let mut matched = false;

    for process in processes {
        if process.pid == self_pid {
            continue;
        }
        if !matches_target(&process, &target) {
            continue;
        }

        // SAFETY: `process.pid` came from the kernel process table, and the
        // signal constant is valid.
        let rc = unsafe { libc::kill(process.pid, libc::SIGTERM) };
        if rc == 0 {
            matched = true;
            continue;
        }

        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            continue;
        }
        return Err(vec![AppletError::from_io(APPLET, "signaling", None, error)]);
    }

    Ok(matched)
}

fn parse_args(args: &[String]) -> Result<String, Vec<AppletError>> {
    match args {
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        [name] => Ok(name.clone()),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn matches_target(process: &crate::common::process::ProcessInfo, target: &str) -> bool {
    process.matches_exact_name(target)
        || (process.name.len() == LINUX_COMM_LIMIT && target.starts_with(&process.name))
}

fn match_exit_code(matched: bool) -> i32 {
    if matched { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use crate::common::process::ProcessInfo;

    use super::{matches_target, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_name() {
        assert_eq!(
            parse_args(&args(&["match-killall-target"])).expect("parse killall"),
            "match-killall-target"
        );
    }

    #[test]
    fn does_not_match_later_command_tokens() {
        let process = ProcessInfo {
            pid: 1,
            ppid: 0,
            uid: 0,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("sh"),
            command: String::from("/bin/sh ./match-killall-target"),
        };
        assert!(!process.matches_exact_name("match-killall-target"));
        assert!(!process.matches_exact_name("killall-target"));
    }

    #[test]
    fn match_exit_codes_follow_killall_convention() {
        assert_eq!(super::match_exit_code(true), 0);
        assert_eq!(super::match_exit_code(false), 1);
    }

    #[test]
    fn matches_truncated_linux_comm_name() {
        let process = ProcessInfo {
            pid: 1,
            ppid: 0,
            uid: 0,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("match-killall-t"),
            command: String::from("/bin/sh ./match-killall-target"),
        };
        assert!(matches_target(&process, "match-killall-target"));
        assert!(!matches_target(&process, "wrapper-killall-target"));
    }
}
