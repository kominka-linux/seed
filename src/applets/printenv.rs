use std::io::Write;
use std::os::unix::ffi::OsStrExt;

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "printenv";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) => code,
        Err(errors) => {
            for e in errors {
                e.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let mut null_terminated = false;
    let mut names: Vec<&str> = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && (arg == "-0" || arg == "--null") {
            null_terminated = true;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        names.push(arg);
    }

    let sep: &[u8] = if null_terminated { b"\0" } else { b"\n" };
    let mut out = stdout();

    if names.is_empty() {
        for (key, value) in std::env::vars_os() {
            out.write_all(key.as_os_str().as_bytes())
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            out.write_all(b"=")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            out.write_all(value.as_os_str().as_bytes())
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            out.write_all(sep)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
        return Ok(0);
    }

    // Print specific variables; exit 1 if any are unset.
    let mut all_found = true;
    for name in &names {
        match std::env::var_os(name) {
            Some(value) => {
                out.write_all(value.as_os_str().as_bytes())
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
                out.write_all(sep)
                    .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
            }
            None => all_found = false,
        }
    }
    Ok(if all_found { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn known_var_exits_zero() {
        // PATH is virtually always set
        assert_eq!(run(&args(&["PATH"])).unwrap(), 0);
    }

    #[test]
    fn unknown_var_exits_one() {
        assert_eq!(run(&args(&["__SEED_DEFINITELY_NOT_SET_XYZ__"])).unwrap(), 1);
    }

    #[test]
    fn no_args_exits_zero() {
        assert_eq!(run(&args(&[])).unwrap(), 0);
    }

    #[test]
    fn mixed_found_and_not_found_exits_one() {
        assert_eq!(
            run(&args(&["PATH", "__SEED_DEFINITELY_NOT_SET_XYZ__"])).unwrap(),
            1
        );
    }
}
