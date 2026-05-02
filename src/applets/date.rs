use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::mem::MaybeUninit;
use std::os::raw::c_char;
use std::time::UNIX_EPOCH;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "date";
const DEFAULT_FORMAT: &str = "%a %b %e %H:%M:%S %Z %Y";
const RFC2822_FORMAT: &str = "%a, %d %b %Y %H:%M:%S %z";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IsoSpec {
    Date,
    Hours,
    Minutes,
    Seconds,
    Ns,
}

#[derive(Debug, Default)]
struct Options {
    utc: bool,
    rfc2822: bool,
    display_time: Option<String>,
    set_time: Option<String>,
    reference_file: Option<String>,
    format: Option<String>,
    input_format: Option<String>,
    iso_spec: Option<IsoSpec>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    let seconds = resolve_time(&options)?;

    if options.set_time.is_some() {
        set_system_time(seconds)?;
    }

    let rendered = if let Some(spec) = options.iso_spec {
        format_iso_time(seconds, spec, options.utc)?
    } else {
        let format = options.format.as_deref().unwrap_or(if options.rfc2822 {
            RFC2822_FORMAT
        } else {
            DEFAULT_FORMAT
        });
        format_time(seconds, format, options.utc)?
    };

    let mut out = stdout();
    writeln!(out, "{rendered}")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::LongOption("date", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "d")?;
                assign_time_source(&mut options.display_time, value.to_owned())?;
            }
            ArgToken::LongOption("set", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "s")?;
                assign_time_source(&mut options.set_time, value.to_owned())?;
            }
            ArgToken::LongOption("reference", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "r")?;
                if options.reference_file.is_some() {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "multiple reference files specified",
                    )]);
                }
                options.reference_file = Some(value.to_owned());
            }
            ArgToken::LongOption("input-format", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "D")?;
                if options.input_format.is_some() {
                    return Err(vec![AppletError::new(
                        APPLET,
                        "multiple input formats specified",
                    )]);
                }
                options.input_format = Some(value.to_owned());
            }
            ArgToken::LongOption("iso-8601", attached) => {
                let spec = match attached {
                    Some(value) if !value.is_empty() => parse_iso_spec(value)?,
                    _ => IsoSpec::Date,
                };
                assign_iso_spec(&mut options, spec)?;
            }
            ArgToken::LongOption("rfc-2822", None) | ArgToken::LongOption("rfc-email", None) => {
                options.rfc2822 = true;
            }
            ArgToken::LongOption("utc", None) => options.utc = true,
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.chars();
                while let Some(flag) = chars.next() {
                    let attached = chars.as_str();
                    match flag {
                        'd' => {
                            let value = cursor.next_value_or_attached(attached, APPLET, "d")?;
                            assign_time_source(&mut options.display_time, value.to_owned())?;
                            break;
                        }
                        's' => {
                            let value = cursor.next_value_or_attached(attached, APPLET, "s")?;
                            assign_time_source(&mut options.set_time, value.to_owned())?;
                            break;
                        }
                        'r' => {
                            let value = cursor.next_value_or_attached(attached, APPLET, "r")?;
                            if options.reference_file.is_some() {
                                return Err(vec![AppletError::new(
                                    APPLET,
                                    "multiple reference files specified",
                                )]);
                            }
                            options.reference_file = Some(value.to_owned());
                            break;
                        }
                        'D' => {
                            let value = cursor.next_value_or_attached(attached, APPLET, "D")?;
                            if options.input_format.is_some() {
                                return Err(vec![AppletError::new(
                                    APPLET,
                                    "multiple input formats specified",
                                )]);
                            }
                            options.input_format = Some(value.to_owned());
                            break;
                        }
                        'I' => {
                            let spec = if attached.is_empty() {
                                IsoSpec::Date
                            } else {
                                parse_iso_spec(attached)?
                            };
                            assign_iso_spec(&mut options, spec)?;
                            break;
                        }
                        'R' => options.rfc2822 = true,
                        'u' => options.utc = true,
                        _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                    }
                }
            }
            ArgToken::Operand(arg) => {
                if let Some(format) = arg.strip_prefix('+') {
                    if options.format.is_some() {
                        return Err(vec![AppletError::new(
                            APPLET,
                            "multiple output formats specified",
                        )]);
                    }
                    options.format = Some(format.to_owned());
                    continue;
                }

                if options.set_time.is_none() {
                    options.set_time = Some(arg.to_owned());
                    continue;
                }

                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unexpected operand '{arg}'"),
                )]);
            }
        }
    }

    let source_count = usize::from(options.display_time.is_some())
        + usize::from(options.set_time.is_some())
        + usize::from(options.reference_file.is_some());
    if source_count > 1 {
        return Err(vec![AppletError::new(
            APPLET,
            "options '-d', '-r', and '-s' are mutually exclusive",
        )]);
    }

    Ok(options)
}

fn assign_time_source(slot: &mut Option<String>, value: String) -> Result<(), Vec<AppletError>> {
    if slot.is_some() {
        return Err(vec![AppletError::new(
            APPLET,
            "multiple time arguments specified",
        )]);
    }
    *slot = Some(value);
    Ok(())
}

fn resolve_time(options: &Options) -> AppletResultTime {
    if let Some(path) = &options.reference_file {
        return file_mtime_seconds(path);
    }
    if let Some(spec) = &options.display_time {
        return parse_time_spec(spec, options.utc, options.input_format.as_deref());
    }
    if let Some(spec) = &options.set_time {
        return parse_time_spec(spec, options.utc, options.input_format.as_deref());
    }
    current_time()
}

fn assign_iso_spec(options: &mut Options, spec: IsoSpec) -> Result<(), Vec<AppletError>> {
    if options.iso_spec.is_some() {
        return Err(vec![AppletError::new(
            APPLET,
            "multiple iso formats specified",
        )]);
    }
    options.iso_spec = Some(spec);
    Ok(())
}

fn parse_iso_spec(spec: &str) -> Result<IsoSpec, Vec<AppletError>> {
    match spec {
        "date" => Ok(IsoSpec::Date),
        "hours" => Ok(IsoSpec::Hours),
        "minutes" => Ok(IsoSpec::Minutes),
        "seconds" => Ok(IsoSpec::Seconds),
        "ns" => Ok(IsoSpec::Ns),
        _ => Err(vec![AppletError::new(
            APPLET,
            format!("invalid argument '{spec}' for '-I'"),
        )]),
    }
}

type AppletResultTime = Result<i64, Vec<AppletError>>;

fn current_time() -> AppletResultTime {
    let mut now: libc::c_long = 0;
    // SAFETY: `time` writes the current epoch seconds into a valid pointer.
    unsafe {
        libc::time(&mut now);
    }
    Ok(now as i64)
}

fn file_mtime_seconds(path: &str) -> AppletResultTime {
    let modified = fs::metadata(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "statting", Some(path), err)])?
        .modified()
        .map_err(|err| {
            vec![AppletError::new(
                APPLET,
                format!("reading {path} mtime: {err}"),
            )]
        })?;
    let duration = modified.duration_since(UNIX_EPOCH).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("{path}: mtime before unix epoch"),
        )]
    })?;
    i64::try_from(duration.as_secs()).map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("{path}: mtime out of range"),
        )]
    })
}

fn parse_time_spec(spec: &str, utc: bool, input_format: Option<&str>) -> AppletResultTime {
    if let Some(epoch) = spec.strip_prefix('@') {
        return epoch.parse::<i64>().map_err(|_| invalid_date(spec));
    }

    if let Some(format) = input_format
        && let Some(seconds) = parse_custom_time_spec(spec, format, utc)?
    {
        return Ok(seconds);
    }

    if let Some(seconds) = parse_time_with_explicit_offset(spec)? {
        return Ok(seconds);
    }

    for candidate in CANDIDATES {
        if let Some(seconds) = parse_with_format(spec, candidate, utc)? {
            return Ok(seconds);
        }
    }

    Err(invalid_date(spec))
}

fn parse_custom_time_spec(spec: &str, format: &str, utc: bool) -> AppletResultOffset {
    let zero_seconds = !format.contains("%S");
    let tm = match parse_tm(spec, format, zero_seconds)? {
        Some(tm) => tm,
        None => return Ok(None),
    };
    let seconds = if utc {
        to_epoch_utc(tm)?
    } else {
        to_epoch_local(tm)?
    };
    Ok(Some(seconds))
}

struct ParseCandidate {
    format: &'static str,
    zero_seconds: bool,
}

const CANDIDATES: &[ParseCandidate] = &[
    ParseCandidate {
        format: "%H:%M:%S",
        zero_seconds: false,
    },
    ParseCandidate {
        format: "%H:%M",
        zero_seconds: true,
    },
    ParseCandidate {
        format: "%m.%d-%H:%M:%S",
        zero_seconds: false,
    },
    ParseCandidate {
        format: "%m.%d-%H:%M",
        zero_seconds: true,
    },
    ParseCandidate {
        format: "%Y.%m.%d-%H:%M:%S",
        zero_seconds: false,
    },
    ParseCandidate {
        format: "%Y.%m.%d-%H:%M",
        zero_seconds: true,
    },
    ParseCandidate {
        format: "%Y-%m-%d %H:%M:%S",
        zero_seconds: false,
    },
    ParseCandidate {
        format: "%Y-%m-%d %H:%M",
        zero_seconds: true,
    },
    ParseCandidate {
        format: "%Y%m%d%H%M.%S",
        zero_seconds: false,
    },
    ParseCandidate {
        format: "%Y%m%d%H%M",
        zero_seconds: true,
    },
];

fn parse_time_with_explicit_offset(spec: &str) -> AppletResultOffset {
    if let Some(prefix) = spec.strip_suffix('Z') {
        let Some(tm) = parse_tm(prefix, "%Y-%m-%d %H:%M:%S", false)? else {
            return Ok(None);
        };
        return Ok(Some(to_epoch_utc(tm)?));
    }

    let Some(split) = spec.rfind(' ') else {
        return Ok(None);
    };
    let prefix = &spec[..split];
    let suffix = &spec[split + 1..];
    let offset = match parse_utc_offset(suffix) {
        Some(offset) => offset,
        None => return Ok(None),
    };
    let Some(tm) = parse_tm(prefix, "%Y-%m-%d %H:%M:%S", false)? else {
        return Ok(None);
    };
    let seconds = to_epoch_utc(tm)?
        .checked_sub(offset)
        .ok_or_else(|| invalid_date(spec))?;
    Ok(Some(seconds))
}

type AppletResultOffset = Result<Option<i64>, Vec<AppletError>>;

fn parse_utc_offset(text: &str) -> Option<i64> {
    let digits = if text.len() == 5 {
        text
    } else if text.len() == 6 && text.as_bytes().get(3) == Some(&b':') {
        let mut compact = String::with_capacity(5);
        compact.push_str(&text[..3]);
        compact.push_str(&text[4..]);
        return parse_utc_offset(&compact);
    } else {
        return None;
    };
    let sign = match digits.as_bytes()[0] {
        b'+' => 1_i64,
        b'-' => -1_i64,
        _ => return None,
    };
    let hours = digits[1..3].parse::<i64>().ok()?;
    let minutes = digits[3..5].parse::<i64>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    let seconds = sign * (hours * 3600 + minutes * 60);
    Some(seconds)
}

fn parse_with_format(spec: &str, candidate: &ParseCandidate, utc: bool) -> AppletResultOffset {
    let tm = match parse_tm(spec, candidate.format, candidate.zero_seconds)? {
        Some(tm) => tm,
        None => return Ok(None),
    };
    let seconds = if utc {
        to_epoch_utc(tm)?
    } else {
        to_epoch_local(tm)?
    };
    Ok(Some(seconds))
}

fn parse_tm(
    spec: &str,
    format: &str,
    zero_seconds: bool,
) -> Result<Option<libc::tm>, Vec<AppletError>> {
    let mut tm = current_tm_struct()?;
    if zero_seconds {
        tm.tm_sec = 0;
    }
    tm.tm_isdst = -1;

    let spec_c = CString::new(spec).map_err(|_| invalid_date(spec))?;
    let format_c = CString::new(format).map_err(|_| invalid_date(spec))?;
    // SAFETY: `strptime` reads valid NUL-terminated strings and writes into
    // a properly initialized `tm` value for the duration of the call.
    let end = unsafe { libc::strptime(spec_c.as_ptr(), format_c.as_ptr(), &mut tm) };
    if end.is_null() {
        return Ok(None);
    }
    // SAFETY: `end` points into `spec_c` when `strptime` succeeds.
    if unsafe { *end } != 0 {
        return Ok(None);
    }
    Ok(Some(tm))
}

fn current_tm_struct() -> Result<libc::tm, Vec<AppletError>> {
    let mut now: libc::c_long = 0;
    // SAFETY: `time` writes the current epoch seconds into a valid pointer.
    unsafe {
        libc::time(&mut now);
    }
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    // SAFETY: `localtime_r` writes a valid `tm` for a valid input pointer.
    let ptr = unsafe { libc::localtime_r(&now, tm.as_mut_ptr()) };
    if ptr.is_null() {
        return Err(vec![AppletError::new(
            APPLET,
            "failed to read current time",
        )]);
    }
    // SAFETY: `localtime_r` initialized `tm` on success.
    Ok(unsafe { tm.assume_init() })
}

fn to_epoch_local(mut tm: libc::tm) -> AppletResultTime {
    tm.tm_isdst = -1;
    // SAFETY: `mktime` consumes a valid `tm` pointer and normalizes it.
    let seconds = unsafe { libc::mktime(&mut tm) };
    if seconds == -1 {
        Err(invalid_date("time"))
    } else {
        Ok(seconds as i64)
    }
}

fn to_epoch_utc(mut tm: libc::tm) -> AppletResultTime {
    tm.tm_isdst = 0;
    // SAFETY: `timegm` consumes a valid UTC `tm` pointer and normalizes it.
    let seconds = unsafe { libc::timegm(&mut tm) };
    if seconds == -1 {
        Err(invalid_date("time"))
    } else {
        Ok(seconds as i64)
    }
}

fn set_system_time(seconds: i64) -> AppletResult {
    #[allow(clippy::useless_conversion)]
    let tv_sec: libc::c_long = libc::c_long::try_from(seconds)
        .map_err(|_| vec![AppletError::new(APPLET, "timestamp out of range")])?;
    let tv = libc::timeval { tv_sec, tv_usec: 0 };
    // SAFETY: `settimeofday` reads a valid `timeval`; the timezone pointer is null.
    let status = unsafe { libc::settimeofday(&tv, std::ptr::null()) };
    if status == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("setting time: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn format_time(seconds: i64, format: &str, utc: bool) -> Result<String, Vec<AppletError>> {
    #[allow(clippy::useless_conversion)]
    let c_seconds: libc::c_long = libc::c_long::try_from(seconds)
        .map_err(|_| vec![AppletError::new(APPLET, "timestamp out of range")])?;
    let format_c = CString::new(format)
        .map_err(|_| vec![AppletError::new(APPLET, "invalid format string")])?;
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    let mut buffer = [0 as c_char; 256];

    // SAFETY: all pointers are valid for the duration of the call, and the
    // formatter writes at most `buffer.len()` bytes including the trailing NUL.
    let written = unsafe {
        let tm_ptr = if utc {
            libc::gmtime_r(&c_seconds, tm.as_mut_ptr())
        } else {
            libc::localtime_r(&c_seconds, tm.as_mut_ptr())
        };
        if tm_ptr.is_null() {
            return Err(vec![AppletError::new(APPLET, "failed to format time")]);
        }
        libc::strftime(
            buffer.as_mut_ptr(),
            buffer.len(),
            format_c.as_ptr(),
            tm.as_ptr(),
        )
    };
    if written == 0 {
        return Err(vec![AppletError::new(APPLET, "failed to format time")]);
    }

    let bytes = buffer[..written]
        .iter()
        .map(|&byte| byte as u8)
        .collect::<Vec<_>>();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn format_iso_time(seconds: i64, spec: IsoSpec, utc: bool) -> Result<String, Vec<AppletError>> {
    let base = match spec {
        IsoSpec::Date => format_time(seconds, "%Y-%m-%d", utc)?,
        IsoSpec::Hours => format_time(seconds, "%Y-%m-%dT%H", utc)?,
        IsoSpec::Minutes => format_time(seconds, "%Y-%m-%dT%H:%M", utc)?,
        IsoSpec::Seconds | IsoSpec::Ns => format_time(seconds, "%Y-%m-%dT%H:%M:%S", utc)?,
    };

    if spec == IsoSpec::Date {
        return Ok(base);
    }

    let mut rendered = base;
    if spec == IsoSpec::Ns {
        rendered.push_str(".000000000");
    }
    rendered.push_str(&format_iso_offset(seconds, utc)?);
    Ok(rendered)
}

fn format_iso_offset(seconds: i64, utc: bool) -> Result<String, Vec<AppletError>> {
    if utc {
        return Ok(String::from("+00:00"));
    }

    let offset = format_time(seconds, "%z", false)?;
    if offset.len() != 5 {
        return Err(vec![AppletError::new(
            APPLET,
            "failed to format timezone offset",
        )]);
    }
    Ok(format!("{}:{}", &offset[..3], &offset[3..]))
}

fn invalid_date(spec: &str) -> Vec<AppletError> {
    vec![AppletError::new(APPLET, format!("invalid date '{spec}'"))]
}

#[cfg(test)]
mod tests {
    use super::{
        IsoSpec, format_iso_time, parse_args, parse_iso_spec, parse_time_spec, parse_utc_offset,
    };
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_epoch_spec() {
        let seconds = parse_time_spec("@0", false, None).expect("parse epoch");
        assert_eq!(seconds, 0);
    }

    #[test]
    fn parses_utc_offset() {
        assert_eq!(parse_utc_offset("+0600"), Some(21_600));
        assert_eq!(parse_utc_offset("+06:00"), Some(21_600));
        assert_eq!(parse_utc_offset("-0130"), Some(-5_400));
        assert_eq!(parse_utc_offset("UTC"), None);
    }

    #[test]
    fn parses_compact_datetime() {
        let seconds = parse_time_spec("200001231133.30", true, None).expect("parse datetime");
        assert!(seconds > 0);
    }

    #[test]
    fn parses_custom_input_format() {
        let seconds = parse_time_spec("1999/01/02-03:04:05", true, Some("%Y/%m/%d-%H:%M:%S"))
            .expect("parse custom datetime");
        assert_eq!(seconds, 915_246_245);
    }

    #[test]
    fn parses_iso_specifiers() {
        assert_eq!(parse_iso_spec("date").expect("date"), IsoSpec::Date);
        assert_eq!(
            parse_iso_spec("seconds").expect("seconds"),
            IsoSpec::Seconds
        );
    }

    #[test]
    fn parses_attached_iso_specifier() {
        let options = parse_args(&args(&["-Iseconds"])).expect("parse date");
        assert_eq!(options.iso_spec, Some(IsoSpec::Seconds));
    }

    #[test]
    fn formats_iso_seconds_in_utc() {
        let rendered = format_iso_time(915_246_245, IsoSpec::Seconds, true).expect("format iso");
        assert_eq!(rendered, "1999-01-02T03:04:05+00:00");
    }
}
