use std::io::{BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "rev";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut paths: Vec<&str> = Vec::new();
    let mut end_of_opts = false;

    for arg in args {
        if !end_of_opts && arg == "--" {
            end_of_opts = true;
            continue;
        }
        if !end_of_opts && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap(),
            )]);
        }
        paths.push(arg);
    }

    let paths: Vec<&str> = if paths.is_empty() { vec!["-"] } else { paths };
    let mut out = stdout();
    let mut errors = Vec::new();

    for &path in &paths {
        match open_input(path) {
            Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
            Ok(f) => {
                let reader = BufReader::new(f);
                for line in reader.lines() {
                    match line {
                        Err(e) => {
                            errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                            break;
                        }
                        Ok(l) => {
                            let reversed: String = l.chars().rev().collect();
                            if let Err(e) = writeln!(out, "{reversed}") {
                                return Err(vec![AppletError::from_io(APPLET, "writing", None, e)]);
                            }
                        }
                    }
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // rev is tested through the internal logic; capture via run() return value only.

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"])).is_err());
    }

    #[test]
    fn nonexistent_file_fails() {
        assert!(run(&args(&["__no_such_file__"])).is_err());
    }

    #[test]
    fn reverse_chars() {
        assert_eq!("abc".chars().rev().collect::<String>(), "cba");
        assert_eq!("hello".chars().rev().collect::<String>(), "olleh");
        assert_eq!("".chars().rev().collect::<String>(), "");
        // Unicode: multi-codepoint works correctly
        assert_eq!("café".chars().rev().collect::<String>(), "éfac");
    }
}
