use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

pub fn main_sync(_: &[String]) -> i32 {
    // SAFETY: sync() has no failure mode.
    unsafe { libc::sync() };
    0
}

pub fn main_fsync(args: &[String]) -> i32 {
    finish(run_fsync(args))
}

fn run_fsync(args: &[String]) -> AppletResult {
    const APPLET: &str = "fsync";
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }
    let mut errors = Vec::new();
    for path in args {
        match OpenOptions::new().read(true).write(true).open(path) {
            Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
            Ok(file) => {
                // SAFETY: file.as_raw_fd() is a valid open file descriptor.
                if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
                    let e = std::io::Error::last_os_error();
                    errors.push(AppletError::from_io(APPLET, "syncing", Some(path), e));
                }
            }
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

#[cfg(test)]
mod tests {
    use super::{main_sync, run_fsync};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sync_returns_success() {
        assert_eq!(main_sync(&[]), 0);
    }

    #[test]
    fn fsync_requires_operand() {
        assert!(run_fsync(&[]).is_err());
    }

    #[test]
    fn fsync_succeeds_for_regular_file() {
        let dir = crate::common::unix::temp_dir("fsync-test");
        let path = dir.join("file");
        std::fs::write(&path, b"data").unwrap();
        run_fsync(&args(&[path.to_str().unwrap()])).unwrap();
        std::fs::remove_dir_all(dir).ok();
    }
}
