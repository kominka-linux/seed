
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::common::applet::finish;
use crate::common::error::AppletError;

const APPLET: &str = "pivot_root";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let (new_root, put_old) = parse_args(args)?;
    run_linux(&new_root, &put_old)
}

fn parse_args(args: &[String]) -> Result<(String, String), Vec<AppletError>> {
    match args {
        [new_root, put_old] => Ok((new_root.clone(), put_old.clone())),
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        [_] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn run_linux(new_root: &str, put_old: &str) -> Result<(), Vec<AppletError>> {
    let new_root = CString::new(Path::new(new_root).as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let put_old = CString::new(Path::new(put_old).as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;

    // SAFETY: both C strings are valid NUL-terminated paths for the duration of the syscall.
    let rc = unsafe { libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("pivot_root: {}", std::io::Error::last_os_error()),
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
    fn parses_new_root_and_put_old() {
        assert_eq!(
            parse_args(&args(&["/newroot", "/newroot/old"])).unwrap(),
            (String::from("/newroot"), String::from("/newroot/old"))
        );
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(parse_args(&args(&["a", "b", "c"])).is_err());
    }
}
