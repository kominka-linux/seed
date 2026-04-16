use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "nproc";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut all = false;
    let mut parser = Parser::new(APPLET, args);
    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Long(name) if name == "all" => all = true,
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(_) => {}
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

    fn args(v: &[&str]) -> Vec<std::ffi::OsString> {
        v.iter().map(std::ffi::OsString::from).collect()
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
