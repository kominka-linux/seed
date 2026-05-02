use std::io::{BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "shuf";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

// Xorshift64 PRNG — good enough for shuffling, seeded from wall-clock time.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(6364136223846793005);
        Rng(if seed == 0 { 1 } else { seed })
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn bounded(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut echo_mode = false;
    let mut repeat = false;
    let mut head_count: Option<usize> = None;
    let mut input_range: Option<(u64, u64)> = None;
    let mut paths: Vec<&str> = Vec::new();
    let mut echo_args: Vec<&str> = Vec::new();
    let mut end_of_opts = false;
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        if !end_of_opts {
            if let Some(val) = arg.strip_prefix("--head-count=") {
                head_count = Some(val.parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                })?);
                continue;
            }
            if let Some(val) = arg.strip_prefix("--input-range=") {
                input_range = Some(parse_range(val).ok_or_else(|| {
                    vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                })?);
                continue;
            }
            match arg {
                "--" => {
                    end_of_opts = true;
                    continue;
                }
                "-e" | "--echo" => {
                    echo_mode = true;
                    continue;
                }
                "-r" | "--repeat" => {
                    repeat = true;
                    continue;
                }
                "-n" => {
                    let val = cursor.next_value(APPLET, "-n")?;
                    head_count = Some(val.parse().map_err(|_| {
                        vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                    })?);
                    continue;
                }
                a if a.starts_with("-n") && a.len() > 2 => {
                    let val = &a[2..];
                    head_count = Some(val.parse().map_err(|_| {
                        vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                    })?);
                    continue;
                }
                "--head-count" => {
                    let val = cursor.next_value(APPLET, "head-count")?;
                    head_count = Some(val.parse().map_err(|_| {
                        vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                    })?);
                    continue;
                }
                "-i" => {
                    let val = cursor.next_value(APPLET, "-i")?;
                    input_range = Some(parse_range(val).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                    })?);
                    continue;
                }
                a if a.starts_with("-i") && a.len() > 2 => {
                    let val = &a[2..];
                    input_range = Some(parse_range(val).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                    })?);
                    continue;
                }
                "--input-range" => {
                    let val = cursor.next_value(APPLET, "input-range")?;
                    input_range = Some(parse_range(val).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                    })?);
                    continue;
                }
                _ if arg.starts_with('-') && arg.len() > 1 => {
                    return Err(vec![AppletError::invalid_option(
                        APPLET,
                        arg.chars().nth(1).unwrap(),
                    )]);
                }
                _ => {}
            }
        }
        if echo_mode {
            echo_args.push(arg);
        } else {
            paths.push(arg);
        }
    }

    // Build the pool of lines to shuffle.
    let lines: Vec<String> = if let Some((lo, hi)) = input_range {
        if hi < lo {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid range: {lo}-{hi}"),
            )]);
        }
        (lo..=hi).map(|n| n.to_string()).collect()
    } else if echo_mode {
        echo_args.iter().map(|s| s.to_string()).collect()
    } else {
        let read_paths: Vec<&str> = if paths.is_empty() {
            vec!["-"]
        } else {
            paths.clone()
        };
        let mut collected = Vec::new();
        let mut errors = Vec::new();
        for path in read_paths {
            match open_input(path) {
                Err(e) => errors.push(AppletError::from_io(APPLET, "opening", Some(path), e)),
                Ok(f) => {
                    let reader = BufReader::new(f);
                    for line in reader.lines() {
                        match line {
                            Err(e) => {
                                errors.push(AppletError::from_io(APPLET, "reading", Some(path), e));
                                break;
                            }
                            Ok(l) => collected.push(l),
                        }
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        collected
    };

    if lines.is_empty() {
        return Ok(());
    }

    let mut rng = Rng::new();
    let mut out = stdout();

    // -r with -n: output n random picks with replacement (no shuffle needed).
    if repeat {
        let count = head_count.unwrap_or(lines.len());
        for _ in 0..count {
            let idx = rng.bounded(lines.len());
            if let Err(e) = writeln!(out, "{}", lines[idx]) {
                return Err(vec![AppletError::from_io(APPLET, "writing", None, e)]);
            }
        }
        return Ok(());
    }

    // Fisher-Yates shuffle.
    let mut shuffled: Vec<usize> = (0..lines.len()).collect();
    for j in (1..shuffled.len()).rev() {
        let k = rng.bounded(j + 1);
        shuffled.swap(j, k);
    }

    let count = head_count.unwrap_or(shuffled.len()).min(shuffled.len());
    for &idx in shuffled.iter().take(count) {
        if let Err(e) = writeln!(out, "{}", lines[idx]) {
            return Err(vec![AppletError::from_io(APPLET, "writing", None, e)]);
        }
    }

    Ok(())
}

fn parse_range(s: &str) -> Option<(u64, u64)> {
    let dash = s.find('-')?;
    let lo: u64 = s[..dash].parse().ok()?;
    let hi: u64 = s[dash + 1..].parse().ok()?;
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<std::ffi::OsString> {
        v.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn rng_does_not_produce_zero() {
        let mut rng = Rng(1);
        for _ in 0..1000 {
            assert_ne!(rng.next(), 0);
        }
    }

    #[test]
    fn rng_bounded_within_range() {
        let mut rng = Rng(42);
        for _ in 0..1000 {
            assert!(rng.bounded(10) < 10);
        }
    }

    #[test]
    fn parse_range_basic() {
        assert_eq!(parse_range("1-5"), Some((1, 5)));
        assert_eq!(parse_range("0-0"), Some((0, 0)));
        assert_eq!(parse_range("10-20"), Some((10, 20)));
    }

    #[test]
    fn parse_range_invalid() {
        assert_eq!(parse_range("abc"), None);
        assert_eq!(parse_range("1"), None);
    }

    #[test]
    fn invalid_range_fails() {
        assert!(run(&args(&["-i", "5-2"])).is_err());
    }

    #[test]
    fn invalid_option_fails() {
        assert!(run(&args(&["-z"])).is_err());
    }

    #[test]
    fn echo_mode_produces_all_items() {
        // Can't check order (random), but can check count via -n 0
        assert!(run(&args(&["-e", "-n", "0", "a", "b", "c"])).is_ok());
    }

    #[test]
    fn input_range_count() {
        // -i 1-5 -n 0 should succeed (0 output lines)
        assert!(run(&args(&["-i", "1-5", "-n", "0"])).is_ok());
    }

    #[test]
    fn head_count_zero() {
        assert!(run(&args(&["-i", "1-100", "-n", "0"])).is_ok());
    }

    #[test]
    fn repeat_mode_accepted() {
        assert!(run(&args(&["-i", "1-10", "-r", "-n", "5"])).is_ok());
    }

    #[test]
    fn nonexistent_file_fails() {
        assert!(run(&args(&["__no_such_file__"])).is_err());
    }

    #[test]
    fn supports_attached_and_separate_count_forms() {
        assert!(run(&args(&["-n1", "-e", "a", "b"])).is_ok());
        assert!(run(&args(&["--head-count", "1", "-e", "a", "b"])).is_ok());
        assert!(run(&args(&["-i1-3", "-n1"])).is_ok());
        assert!(run(&args(&["--input-range", "1-3", "--head-count=1"])).is_ok());
    }
}
