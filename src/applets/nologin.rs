use std::fs::File;
use std::io::{self, Write};

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::error::AppletError;
use crate::common::io::copy_stream;

const APPLET: &str = "nologin";
const NOLOGIN_PATH: &str = "/etc/nologin.txt";
const DEFAULT_MESSAGE: &str = "This account is currently not available.";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    validate_args(args)?;

    match File::open(NOLOGIN_PATH) {
        Ok(mut file) => {
            let mut out = io::stdout().lock();
            copy_stream(&mut file, &mut out).map_err(|err| io_error("writing", err))?;
            out.flush().map_err(|err| io_error("writing", err))?;
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(vec![AppletError::new(APPLET, DEFAULT_MESSAGE)]);
        }
        Err(err) => return Err(io_error("reading", err)),
    }

    Ok(1)
}

fn validate_args(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    match args {
        [] => Ok(()),
        [arg] if arg == "--" => Ok(()),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn io_error(action: &str, err: io::Error) -> Vec<AppletError> {
    vec![AppletError::from_io(APPLET, action, Some(NOLOGIN_PATH), err)]
}

#[cfg(test)]
mod tests {
    use super::validate_args;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn accepts_no_operands() {
        assert!(validate_args(&[]).is_ok());
        assert!(validate_args(&args(&["--"])).is_ok());
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(validate_args(&args(&["user"])).is_err());
        assert!(validate_args(&args(&["--", "user"])).is_err());
    }
}
