use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "seq";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut equal_width = false;
    let mut separator = "\n".to_string();
    let mut format: Option<&str> = None;
    let mut positionals: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    positionals.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-w" | "--equal-width" => equal_width = true,
            "-s" | "--separator" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "s")]);
                }
                separator = unescape(&args[i]);
            }
            "-f" | "--format" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "f")]);
                }
                format = Some(&args[i]);
            }
            // Negative numbers are positional operands, not flags
            a if a.starts_with('-') && a.len() > 1 && !looks_like_number(a) => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => positionals.push(arg),
        }
        i += 1;
    }

    let (first, incr, last) = match positionals.as_slice() {
        [last] => (1.0_f64, 1.0_f64, parse_f64(last)?),
        [first, last] => (parse_f64(first)?, 1.0_f64, parse_f64(last)?),
        [first, incr, last] => (parse_f64(first)?, parse_f64(incr)?, parse_f64(last)?),
        [] => return Err(vec![AppletError::new(APPLET, "missing operand")]),
        _ => return Err(vec![AppletError::new(APPLET, "too many operands")]),
    };

    if incr == 0.0 {
        return Err(vec![AppletError::new(APPLET, "increment cannot be zero")]);
    }

    // Count values to emit
    let count = if incr > 0.0 {
        ((last - first) / incr).floor() as i64 + 1
    } else {
        ((first - last) / (-incr)).floor() as i64 + 1
    };
    if count <= 0 {
        return Ok(());
    }

    // Infer decimal precision from the input strings
    let prec = if format.is_none() {
        let p0 = decimal_places(positionals[0]);
        let p_incr = positionals.get(1).map(|s| decimal_places(s)).unwrap_or(0);
        let p_last = positionals.last().map(|s| decimal_places(s)).unwrap_or(0);
        p0.max(p_incr).max(p_last)
    } else {
        0 // format string controls output
    };

    // For -w: find the width of the widest formatted value
    let width = if equal_width && format.is_none() {
        let last_val = first + incr * (count - 1) as f64;
        let w_first = format_val(first, prec).len();
        let w_last = format_val(last_val, prec).len();
        w_first.max(w_last)
    } else {
        0
    };

    let mut out = stdout();

    for k in 0..count {
        let val = first + incr * k as f64;
        // Avoid floating-point drift on the last step
        let val = if k == count - 1 { last } else { val };

        let s = if let Some(fmt) = format {
            apply_format(fmt, val)
        } else if equal_width {
            zero_pad(format_val(val, prec), width)
        } else {
            format_val(val, prec)
        };

        out.write_all(s.as_bytes())
            .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        if k < count - 1 {
            out.write_all(separator.as_bytes())
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else {
            out.write_all(b"\n")
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        }
    }
    Ok(())
}

fn looks_like_number(s: &str) -> bool {
    let s = s.trim_start_matches('-').trim_start_matches('+');
    s.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

fn parse_f64(s: &str) -> Result<f64, Vec<AppletError>> {
    s.parse::<f64>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid floating point argument: '{s}'"),
        )]
    })
}

fn decimal_places(s: &str) -> usize {
    // Strip leading sign or exponent for simplicity
    match s.find('.') {
        None => 0,
        Some(pos) => {
            // Only count digits after '.', stop at 'e'/'E'
            s[pos + 1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .count()
        }
    }
}

fn format_val(val: f64, prec: usize) -> String {
    if prec == 0 {
        format!("{val:.0}")
    } else {
        format!("{val:.prec$}")
    }
}

fn zero_pad(mut s: String, width: usize) -> String {
    if s.len() >= width {
        return s;
    }
    let pad = width - s.len();
    if s.starts_with('-') {
        s.insert_str(1, &"0".repeat(pad));
    } else {
        s.insert_str(0, &"0".repeat(pad));
    }
    s
}

// Minimal %f/%g/%e format string support
fn apply_format(fmt: &str, val: f64) -> String {
    // Find the conversion specifier
    let Some(pct) = fmt.find('%') else {
        return fmt.replace("%", "") + &format!("{val}");
    };
    let spec_start = pct + 1;
    // Skip flags, width, precision
    let mut i = spec_start;
    let bytes = fmt.as_bytes();
    while i < bytes.len()
        && (bytes[i] == b'-' || bytes[i] == b'+' || bytes[i] == b' ' || bytes[i] == b'0')
    {
        i += 1;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let prec: usize = if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        fmt[start..i].parse().unwrap_or(6)
    } else {
        6
    };
    let conv = bytes.get(i).copied().unwrap_or(b'g');
    let formatted = match conv {
        b'f' | b'F' => format!("{val:.prec$}"),
        b'e' => format!("{val:.prec$e}"),
        b'E' => format!("{val:.prec$E}"),
        _ => {
            // %g: use shorter of %e/%f
            if val.abs() < 1e-4 || val.abs() >= 10f64.powi(prec as i32) {
                format!("{val:.prec$e}")
            } else {
                // Trim trailing zeros
                let s = format!("{val:.prec$}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
    };
    format!("{}{}{}", &fmt[..pct], formatted, &fmt[i + 1..])
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        apply_format, decimal_places, format_val, looks_like_number, run, unescape, zero_pad,
    };

    #[test]
    fn decimal_precision() {
        assert_eq!(decimal_places("1"), 0);
        assert_eq!(decimal_places("1.5"), 1);
        assert_eq!(decimal_places("1.00"), 2);
    }

    #[test]
    fn format_integer() {
        assert_eq!(format_val(3.0, 0), "3");
        assert_eq!(format_val(-1.0, 0), "-1");
    }

    #[test]
    fn format_float() {
        assert_eq!(format_val(1.5, 1), "1.5");
        assert_eq!(format_val(1.0, 2), "1.00");
    }

    #[test]
    fn zero_padding() {
        assert_eq!(zero_pad("3".to_string(), 3), "003");
        assert_eq!(zero_pad("-3".to_string(), 4), "-003");
        assert_eq!(zero_pad("10".to_string(), 3), "010");
        assert_eq!(zero_pad("100".to_string(), 2), "100"); // already wide enough
    }

    #[test]
    fn run_returns_ok_for_empty_range() {
        let args: Vec<String> = vec!["5".to_string(), "3".to_string()];
        assert!(run(&args).is_ok());
    }

    #[test]
    fn run_errors_on_zero_increment() {
        let args: Vec<String> = vec!["1".to_string(), "0".to_string(), "5".to_string()];
        assert!(run(&args).is_err());
    }

    #[test]
    fn run_errors_on_no_operands() {
        assert!(run(&[]).is_err());
    }

    #[test]
    fn format_strings_are_applied() {
        assert_eq!(apply_format("%.2f", 1.5), "1.50");
        assert_eq!(apply_format("x=%e", 1200.0), "x=1.200000e3");
    }

    #[test]
    fn separator_unescapes_common_sequences() {
        assert_eq!(unescape("\\t"), "\t");
        assert_eq!(unescape("a\\nb"), "a\nb");
        assert_eq!(unescape("\\\\"), "\\");
    }

    #[test]
    fn negative_numbers_are_not_treated_as_flags() {
        assert!(looks_like_number("-2"));
        assert!(looks_like_number("-.5"));
        assert!(!looks_like_number("-x"));
    }
}
