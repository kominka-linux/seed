use std::io::Write;

use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::list_processes;

const APPLET: &str = "pgrep";

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
    let mut out = stdout();
    let mut matched = false;

    for process in processes {
        if process_matches(&process.name, &process.command, &pattern) {
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

fn process_matches(name: &str, command: &str, pattern: &str) -> bool {
    name.contains(pattern) || command.contains(pattern)
}

#[cfg(test)]
mod tests {
    use super::{parse_args, process_matches};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_pattern() {
        assert_eq!(parse_args(&args(&["sleep"])).expect("parse pgrep"), "sleep");
    }

    #[test]
    fn matches_name_or_command() {
        assert!(process_matches("sleep", "/bin/sleep", "sleep"));
        assert!(process_matches("seed", "/bin/sleep", "sleep"));
        assert!(!process_matches("seed", "/usr/bin/python3", "sleep"));
    }
}
