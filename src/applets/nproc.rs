use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "nproc";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut all = false;
    for arg in args {
        match arg.as_str() {
            "--all" => all = true,
            "--" => break,
            a if a.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => {}
        }
    }

    let conf = if all {
        libc::_SC_NPROCESSORS_CONF // total installed CPUs
    } else {
        libc::_SC_NPROCESSORS_ONLN // available (online) CPUs
    };

    // SAFETY: sysconf with a valid constant is always safe; treat <= 0 as 1.
    let count = unsafe { libc::sysconf(conf) }.max(1);

    let mut out = stdout();
    writeln!(out, "{count}").map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accepts_all_flag() {
        assert!(run(&args(&["--all"])).is_ok());
    }

    #[test]
    fn rejects_unknown_flag() {
        assert!(run(&args(&["-x"])).is_err());
    }
}
