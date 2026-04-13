use std::fs::OpenOptions;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "truncate";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut size_spec: Option<&str> = None;
    let mut no_create = false;
    let mut paths: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    paths.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-c" | "--no-create" => no_create = true,
            "-s" | "--size" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "s")]);
                }
                size_spec = Some(&args[i]);
            }
            a if a.starts_with("--size=") => size_spec = Some(&a["--size=".len()..]),
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => paths.push(arg),
        }
        i += 1;
    }

    let spec = size_spec.ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            "you must specify either --size or --reference",
        )]
    })?;

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing file operand")]);
    }

    let mut errors = Vec::new();
    for path in &paths {
        if let Err(e) = truncate_file(path, spec, no_create) {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn truncate_file(path: &str, spec: &str, no_create: bool) -> Result<(), AppletError> {
    let (op, size_str) = parse_size_spec(spec);
    let size = parse_bytes(size_str)
        .ok_or_else(|| AppletError::new(APPLET, format!("invalid size: '{spec}'")))?;

    if no_create && !std::path::Path::new(path).exists() {
        return Ok(());
    }

    let file = OpenOptions::new()
        .write(true)
        .create(!no_create)
        .open(path)
        .map_err(|e| AppletError::from_io(APPLET, "opening", Some(path), e))?;

    let current_len = file
        .metadata()
        .map_err(|e| AppletError::from_io(APPLET, "stat", Some(path), e))?
        .len();

    let new_size = match op {
        SizeOp::Set => size,
        SizeOp::Extend => current_len.saturating_add(size),
        SizeOp::Reduce => current_len.saturating_sub(size),
        SizeOp::AtMost => current_len.min(size),
        SizeOp::AtLeast => current_len.max(size),
        SizeOp::RoundDown => {
            if size == 0 {
                0
            } else {
                (current_len / size) * size
            }
        }
        SizeOp::RoundUp => {
            if size == 0 {
                0
            } else {
                current_len.div_ceil(size) * size
            }
        }
    };

    file.set_len(new_size)
        .map_err(|e| AppletError::from_io(APPLET, "truncating", Some(path), e))
}

#[derive(Copy, Clone)]
enum SizeOp {
    Set,
    Extend,
    Reduce,
    AtMost,
    AtLeast,
    RoundDown,
    RoundUp,
}

fn parse_size_spec(spec: &str) -> (SizeOp, &str) {
    match spec.as_bytes().first() {
        Some(b'+') => (SizeOp::Extend, &spec[1..]),
        Some(b'-') => (SizeOp::Reduce, &spec[1..]),
        Some(b'<') => (SizeOp::AtMost, &spec[1..]),
        Some(b'>') => (SizeOp::AtLeast, &spec[1..]),
        Some(b'/') => (SizeOp::RoundDown, &spec[1..]),
        Some(b'%') => (SizeOp::RoundUp, &spec[1..]),
        _ => (SizeOp::Set, spec),
    }
}

fn parse_bytes(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    // Find where digits end
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let n: u64 = s[..split].parse().ok()?;
    let mult = match &s[split..] {
        "" | "B" => 1u64,
        "K" | "KB" => 1_000,
        "k" | "KiB" => 1_024,
        "M" | "MB" => 1_000_000,
        "m" | "MiB" => 1_048_576,
        "G" | "GB" => 1_000_000_000,
        "g" | "GiB" => 1_073_741_824,
        "T" | "TB" => 1_000_000_000_000,
        "t" | "TiB" => 1_099_511_627_776,
        _ => return None,
    };
    n.checked_mul(mult)
}

#[cfg(test)]
mod tests {
    use super::{SizeOp, parse_bytes, parse_size_spec};

    #[test]
    fn plain_bytes() {
        assert_eq!(parse_bytes("0"), Some(0));
        assert_eq!(parse_bytes("1024"), Some(1024));
        assert_eq!(parse_bytes(""), None);
    }

    #[test]
    fn si_suffixes() {
        assert_eq!(parse_bytes("1K"), Some(1_000));
        assert_eq!(parse_bytes("1M"), Some(1_000_000));
        assert_eq!(parse_bytes("1G"), Some(1_000_000_000));
    }

    #[test]
    fn iec_suffixes() {
        assert_eq!(parse_bytes("1k"), Some(1_024));
        assert_eq!(parse_bytes("2k"), Some(2_048));
        assert_eq!(parse_bytes("1KiB"), Some(1_024));
    }

    #[test]
    fn unknown_suffix() {
        assert_eq!(parse_bytes("1Z"), None);
    }

    #[test]
    fn size_op_prefixes() {
        assert!(matches!(parse_size_spec("+100"), (SizeOp::Extend, "100")));
        assert!(matches!(parse_size_spec("-100"), (SizeOp::Reduce, "100")));
        assert!(matches!(parse_size_spec("<100"), (SizeOp::AtMost, "100")));
        assert!(matches!(parse_size_spec(">100"), (SizeOp::AtLeast, "100")));
        assert!(matches!(
            parse_size_spec("/100"),
            (SizeOp::RoundDown, "100")
        ));
        assert!(matches!(parse_size_spec("%100"), (SizeOp::RoundUp, "100")));
        assert!(matches!(parse_size_spec("100"), (SizeOp::Set, "100")));
    }

    #[test]
    fn filesystem_truncate() {
        let dir = crate::common::unix::temp_dir("truncate-test");
        let path = dir.join("f");
        std::fs::write(&path, b"hello world").unwrap();

        super::truncate_file(path.to_str().unwrap(), "5", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");

        // Extend by 3 bytes (null bytes appended)
        super::truncate_file(path.to_str().unwrap(), "+3", false).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 8);

        std::fs::remove_dir_all(dir).ok();
    }
}
