use std::io::Write;

use crate::common::applet::finish_code;
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::list_processes;

const APPLET: &str = "pgrep";

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args).map(i32::from))
}

fn run(args: &[String]) -> Result<bool, Vec<AppletError>> {
    let pattern = parse_args(args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let self_pid = std::process::id() as i32;
    let mut out = stdout();
    let mut matched = false;

    for process in processes {
        if process.pid != self_pid && process.matches_pattern(&pattern) {
            writeln!(out, "{}", process.pid)
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
            matched = true;
        }
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
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
    use crate::common::process::ProcessInfo;

    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_pattern() {
        assert_eq!(parse_args(&args(&["sleep"])).expect("parse pgrep"), "sleep");
    }

    #[test]
    fn matches_name_or_command() {
        let process = ProcessInfo {
            pid: 1,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("seed"),
            command: String::from("./match-pkill-target"),
        };
        assert!(process.matches_pattern("seed"));
        assert!(process.matches_pattern("pkill-target"));
        assert!(!process.matches_pattern("sleep"));
    }
}
