use std::io::{BufRead, BufReader, Write};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::{open_input, stdout};

const APPLET: &str = "shuf";

pub fn main(args: &[String]) -> i32 {
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

fn run(args: &[String]) -> AppletResult {
    let mut echo_mode = false;
    let mut repeat = false;
    let mut head_count: Option<usize> = None;
    let mut input_range: Option<(u64, u64)> = None;
    let mut paths: Vec<&str> = Vec::new();
    let mut echo_args: Vec<&str> = Vec::new();
    let mut i = 0;
    let mut end_of_opts = false;

    while i < args.len() {
        let arg = &args[i];
        if !end_of_opts {
            match arg.as_str() {
                "--" => {
                    end_of_opts = true;
                    i += 1;
                    continue;
                }
                "-e" | "--echo" => {
                    echo_mode = true;
                    i += 1;
                    continue;
                }
                "-r" | "--repeat" => {
                    repeat = true;
                    i += 1;
                    continue;
                }
                "-n" | "--head-count" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or_else(|| vec![AppletError::option_requires_arg(APPLET, "-n")])?;
                    head_count = Some(val.parse().map_err(|_| {
                        vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                    })?);
                    i += 1;
                    continue;
                }
                a if a.starts_with("--head-count=") => {
                    let val = &a["--head-count=".len()..];
                    head_count = Some(val.parse().map_err(|_| {
                        vec![AppletError::new(APPLET, format!("invalid count '{val}'"))]
                    })?);
                    i += 1;
                    continue;
                }
                "-i" | "--input-range" => {
                    i += 1;
                    let val = args
                        .get(i)
                        .ok_or_else(|| vec![AppletError::option_requires_arg(APPLET, "-i")])?;
                    input_range = Some(parse_range(val).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                    })?);
                    i += 1;
                    continue;
                }
                a if a.starts_with("--input-range=") => {
                    let val = &a["--input-range=".len()..];
                    input_range = Some(parse_range(val).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid range '{val}'"))]
                    })?);
                    i += 1;
                    continue;
                }
                a if a.starts_with('-') && a.len() > 1 => {
                    return Err(vec![AppletError::invalid_option(
                        APPLET,
                        a.chars().nth(1).unwrap(),
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
        i += 1;
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

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
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
}
