use std::fs;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET_LINK: &str = "link";
const APPLET_UNLINK: &str = "unlink";

pub fn main_link(args: &[String]) -> i32 {
    finish(run_link(args))
}

pub fn main_unlink(args: &[String]) -> i32 {
    finish(run_unlink(args))
}

fn run_link(args: &[String]) -> AppletResult {
    match args {
        [src, dst] => fs::hard_link(src, dst).map_err(|e| {
            vec![AppletError::from_io(
                APPLET_LINK,
                "creating link",
                Some(dst),
                e,
            )]
        }),
        [] => Err(vec![AppletError::new(APPLET_LINK, "missing operand")]),
        [_] => Err(vec![AppletError::new(
            APPLET_LINK,
            "missing destination file operand",
        )]),
        _ => Err(vec![AppletError::new(APPLET_LINK, "extra operand")]),
    }
}

fn run_unlink(args: &[String]) -> AppletResult {
    match args {
        [path] => fs::remove_file(path).map_err(|e| {
            vec![AppletError::from_io(
                APPLET_UNLINK,
                "unlinking",
                Some(path),
                e,
            )]
        }),
        [] => Err(vec![AppletError::new(APPLET_UNLINK, "missing operand")]),
        _ => Err(vec![AppletError::new(APPLET_UNLINK, "extra operand")]),
    }
}

#[cfg(test)]
mod tests {
    use super::{run_link, run_unlink};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn link_creates_hard_link() {
        let dir = crate::common::unix::temp_dir("link-test");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::write(&src, b"hello").unwrap();

        run_link(&args(&[src.to_str().unwrap(), dst.to_str().unwrap()])).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");

        // Both point to the same inode
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(&src).unwrap().ino(),
            std::fs::metadata(&dst).unwrap().ino(),
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unlink_removes_file() {
        let dir = crate::common::unix::temp_dir("unlink-test");
        let path = dir.join("f");
        std::fs::write(&path, b"x").unwrap();

        run_unlink(&args(&[path.to_str().unwrap()])).unwrap();
        assert!(!path.exists());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn link_missing_arg() {
        assert!(run_link(&args(&["only-one"])).is_err());
    }

    #[test]
    fn unlink_missing_arg() {
        assert!(run_unlink(&args(&[])).is_err());
    }
}
