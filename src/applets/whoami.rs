use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::unix::lookup_user;

const APPLET: &str = "whoami";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    for arg in args {
        if arg.starts_with('-') && arg != "--" {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
    }

    // SAFETY: getuid() always succeeds.
    let uid = unsafe { libc::getuid() };
    let name = lookup_user(uid);

    let mut out = stdout();
    writeln!(out, "{name}").map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::common::unix::lookup_user;
    use std::ffi::OsString;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn current_user_name_is_non_empty() {
        let uid = unsafe { libc::getuid() };
        assert!(!lookup_user(uid).is_empty());
    }

    #[test]
    fn invalid_option_is_rejected() {
        assert!(run(&args(&["-x"])).is_err());
    }
}
