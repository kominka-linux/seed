use crate::common::error::AppletError;
use crate::common::process::list_processes;

const APPLET: &str = "pkill";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<bool, Vec<AppletError>> {
    let pattern = parse_args(args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let self_pid = std::process::id() as i32;
    let mut matched = false;

    for process in processes {
        if process.pid == self_pid || !process.matches_pattern(&pattern) {
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
        [pattern] => Ok(pattern.clone()),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_pattern() {
        assert_eq!(
            parse_args(&args(&["target"])).expect("parse pkill"),
            "target"
        );
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(parse_args(&args(&["one", "two"])).is_err());
    }
}
