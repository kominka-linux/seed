use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsRawFd;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "insmod";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (path, params) = parse_args(args)?;
    run_linux(&path, &params)
}

fn parse_args(args: &[String]) -> Result<(String, Vec<String>), Vec<AppletError>> {
    let Some(path) = args.first() else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    Ok((path.clone(), args[1..].to_vec()))
}

fn run_linux(path: &str, params: &[String]) -> AppletResult {
    let file = File::open(path)
        .map_err(|err| vec![AppletError::new(APPLET, format!("can't insert '{path}': {err}"))])?;

    let size = file
        .metadata()
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(path), err)])?
        .len();
    if size == 0 {
        return Err(vec![AppletError::new(APPLET, "short read")]);
    }

    let params = params.join(" ");
    let params = CString::new(params)
        .map_err(|_| vec![AppletError::new(APPLET, "module parameters contain NUL byte")])?;

    // SAFETY: the fd is valid and open for reading, `params` is a valid
    // NUL-terminated string, and the final flags argument is zero.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_finit_module,
            file.as_raw_fd(),
            params.as_ptr(),
            0,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        Err(vec![AppletError::new(
            APPLET,
            format!("can't insert '{path}': {err}"),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_path_and_params() {
        let (path, params) =
            parse_args(&args(&["module.ko", "key=value", "other=1"])).expect("parse insmod");
        assert_eq!(path, "module.ko");
        assert_eq!(params, vec![String::from("key=value"), String::from("other=1")]);
    }

    #[test]
    fn missing_operand_errors() {
        assert!(parse_args(&[]).is_err());
    }
}
