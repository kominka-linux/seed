use std::thread;
use std::time::Duration;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "sleep";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let args = argv_to_strings(APPLET, args)?;
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let total = args.iter().try_fold(0.0_f64, |acc, arg| {
        parse_duration(arg).map(|value| acc + value)
    })?;
    thread::sleep(Duration::from_secs_f64(total));
    Ok(())
}

pub(crate) fn parse_duration(arg: &str) -> Result<f64, Vec<AppletError>> {
    let (number, multiplier) = match arg.chars().last() {
        Some('s') => (&arg[..arg.len() - 1], 1.0),
        Some('m') => (&arg[..arg.len() - 1], 60.0),
        Some('h') => (&arg[..arg.len() - 1], 3600.0),
        Some('d') => (&arg[..arg.len() - 1], 86_400.0),
        _ => (arg, 1.0),
    };

    let value = number.parse::<f64>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid time interval '{arg}'"),
        )]
    })?;
    if !value.is_finite() || value < 0.0 {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid time interval '{arg}'"),
        )]);
    }
    Ok(value * multiplier)
}

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn parses_suffixes() {
        assert_eq!(parse_duration("1.5").expect("seconds"), 1.5);
        assert_eq!(parse_duration("2m").expect("minutes"), 120.0);
        assert_eq!(parse_duration("3h").expect("hours"), 10_800.0);
        assert_eq!(parse_duration("1d").expect("days"), 86_400.0);
    }
}
