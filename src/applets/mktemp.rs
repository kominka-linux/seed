use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "mktemp";
const RANDOM_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut dir_mode = false;
    let mut quiet = false;
    let mut dry_run = false;
    let mut tmpdir_opt: Option<PathBuf> = None;
    let mut template: Option<&str> = None;
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-d" | "--directory" => dir_mode = true,
            "-q" | "--quiet" => quiet = true,
            "-u" | "--dry-run" => dry_run = true,
            "-p" => {
                tmpdir_opt = Some(PathBuf::from(cursor.next_value(APPLET, "p")?));
            }
            "--tmpdir" => {
                tmpdir_opt = Some(PathBuf::from(cursor.next_value(APPLET, "tmpdir")?));
            }
            a if a.starts_with("--tmpdir=") => {
                tmpdir_opt = Some(PathBuf::from(&a["--tmpdir=".len()..]));
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => {
                template = Some(arg);
            }
        }
    }

    let tmpl = template.unwrap_or("tmp.XXXXXXXXXX");

    let x_count = tmpl.bytes().rev().take_while(|&b| b == b'X').count();
    if x_count < 3 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("too few X's in template '{tmpl}'"),
        )]);
    }

    let base_dir = tmpdir_opt.unwrap_or_else(|| {
        std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    });

    let prefix = &tmpl[..tmpl.len() - x_count];

    let path = create_unique(&base_dir, prefix, x_count, dir_mode).map_err(|e| {
        if quiet {
            vec![]
        } else {
            vec![AppletError::from_io(APPLET, "creating temp path", None, e)]
        }
    })?;

    if dry_run {
        if dir_mode {
            std::fs::remove_dir(&path).ok();
        } else {
            std::fs::remove_file(&path).ok();
        }
    }

    let mut out = stdout();
    writeln!(out, "{}", path.display())
        .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    Ok(())
}

pub(crate) fn create_unique(
    dir: &Path,
    prefix: &str,
    x_count: usize,
    is_dir: bool,
) -> std::io::Result<PathBuf> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64
        ^ (u64::from(std::process::id()) << 17);

    let mut rng = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);

    for _ in 0..1000 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);

        let suffix: String = (0..x_count)
            .map(|j| {
                let idx = ((rng >> (j * 6 % 58)) as usize) % RANDOM_CHARS.len();
                RANDOM_CHARS[idx] as char
            })
            .collect();

        let path = dir.join(format!("{prefix}{suffix}"));

        if is_dir {
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        } else {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "failed to create unique temporary path",
    ))
}

#[cfg(test)]
mod tests {
    use super::{create_unique, run};

    fn args(v: &[&str]) -> Vec<std::ffi::OsString> {
        v.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn creates_temp_file() {
        let dir = crate::common::unix::temp_dir("mktemp-test");
        let path = create_unique(&dir, "tmp.", 6, false).unwrap();
        assert!(path.exists());
        assert!(path.is_file());
        // Name starts with the prefix
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("tmp.")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn creates_temp_dir() {
        let dir = crate::common::unix::temp_dir("mktemp-test");
        let path = create_unique(&dir, "d.", 6, true).unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn two_calls_produce_distinct_paths() {
        let dir = crate::common::unix::temp_dir("mktemp-test");
        let a = create_unique(&dir, "t.", 8, false).unwrap();
        let b = create_unique(&dir, "t.", 8, false).unwrap();
        assert_ne!(a, b);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_templates_with_too_few_placeholders() {
        let err = run(&args(&["abXX"])).unwrap_err();
        assert_eq!(err.len(), 1);
    }

    #[test]
    fn dry_run_removes_created_path() {
        let dir = crate::common::unix::temp_dir("mktemp-test");
        let template = dir.join("temp.XXXXXX");
        run(&args(&["-u", template.to_str().unwrap()])).unwrap();
        let mut entries = std::fs::read_dir(&dir).unwrap();
        assert!(entries.next().is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
