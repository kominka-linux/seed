use crate::common::applet::finish_code;
use crate::common::error::AppletError;
use crate::common::process::list_processes;

const APPLET: &str = "killall";

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args).map(i32::from))
}

fn run(args: &[String]) -> Result<bool, Vec<AppletError>> {
    let target = parse_args(args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let self_pid = std::process::id() as i32;
    let mut matched = false;

    for process in processes {
        if process.pid == self_pid {
            continue;
        }
        if !process.matches_exact_name(&target) {
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

#[cfg(test)]
mod tests {
    use crate::common::process::ProcessInfo;

    use super::parse_args;

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
    fn matches_exact_name_against_command_tokens() {
        let process = ProcessInfo {
            pid: 1,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("sh"),
            command: String::from("/bin/sh ./match-killall-target"),
        };
        assert!(process.matches_exact_name("match-killall-target"));
        assert!(!process.matches_exact_name("killall-target"));
    }
}
