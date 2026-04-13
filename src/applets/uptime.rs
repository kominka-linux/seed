use std::ffi::CString;
use std::io::Write;
use std::mem::MaybeUninit;

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "uptime";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    parse_args(args)?;
    let snapshot = collect_snapshot()?;
    let mut out = stdout();
    writeln!(out, "{}", format_snapshot(&snapshot))
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(), Vec<AppletError>> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(vec![AppletError::new(APPLET, "extra operand")])
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    now: i64,
    uptime_seconds: u64,
    users: usize,
    loads: [f64; 3],
}

fn format_snapshot(snapshot: &Snapshot) -> String {
    let time = format_clock(snapshot.now).unwrap_or_else(|_| String::from("00:00"));
    let uptime = format_uptime(snapshot.uptime_seconds);
    let user_label = if snapshot.users == 1 { "user" } else { "users" };
    format!(
        "{time}  up {uptime}, {} {user_label}, load averages: {:.2} {:.2} {:.2}",
        snapshot.users, snapshot.loads[0], snapshot.loads[1], snapshot.loads[2]
    )
}

fn format_uptime(uptime_seconds: u64) -> String {
    let total_minutes = (uptime_seconds + 30) / 60;
    if total_minutes < 60 {
        return format!(
            "{total_minutes} min{}",
            if total_minutes == 1 { "" } else { "s" }
        );
    }

    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours < 24 {
        return format!("{hours}:{minutes:02}");
    }

    let days = hours / 24;
    let remaining_hours = hours % 24;
    format!(
        "{days} day{}, {remaining_hours}:{minutes:02}",
        if days == 1 { "" } else { "s" }
    )
}

fn format_clock(seconds: i64) -> Result<String, AppletError> {
    let c_seconds: libc::time_t = seconds;
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    let format = CString::new("%H:%M").expect("static format is NUL-free");
    let mut buffer = [0_i8; 16];

    // SAFETY: `c_seconds`, `tm`, `buffer`, and `format` point to valid memory
    // for the duration of these libc calls, and `format` is NUL-terminated.
    let written = unsafe {
        if libc::localtime_r(&c_seconds, tm.as_mut_ptr()).is_null() {
            return Err(AppletError::new(APPLET, "failed to format time"));
        }
        libc::strftime(
            buffer.as_mut_ptr(),
            buffer.len(),
            format.as_ptr(),
            tm.as_ptr(),
        )
    };
    if written == 0 {
        return Err(AppletError::new(APPLET, "failed to format time"));
    }

    let bytes = buffer[..written]
        .iter()
        .map(|byte| *byte as u8)
        .collect::<Vec<_>>();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "macos")]
fn collect_snapshot() -> Result<Snapshot, Vec<AppletError>> {
    let now = current_time()?;
    let boot = boot_time()?;
    let users = user_count()?;
    let loads = load_averages()?;
    let uptime_seconds = now.saturating_sub(boot) as u64;

    Ok(Snapshot {
        now,
        uptime_seconds,
        users,
        loads,
    })
}

#[cfg(not(target_os = "macos"))]
fn collect_snapshot() -> Result<Snapshot, Vec<AppletError>> {
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

#[cfg(target_os = "macos")]
fn current_time() -> Result<i64, Vec<AppletError>> {
    let mut now: libc::time_t = 0;
    // SAFETY: `now` points to writable memory for `time` to initialize.
    let result = unsafe { libc::time(&mut now) };
    if result == -1 {
        return Err(vec![AppletError::new(
            APPLET,
            "reading current time failed",
        )]);
    }
    Ok(now as i64)
}

#[cfg(target_os = "macos")]
fn boot_time() -> Result<i64, Vec<AppletError>> {
    let name = CString::new("kern.boottime").expect("static string is NUL-free");
    let mut boot_time = MaybeUninit::<libc::timeval>::uninit();
    let mut size = std::mem::size_of::<libc::timeval>();

    // SAFETY: `name` is NUL-terminated, `boot_time` points to writable memory,
    // and `size` describes that buffer accurately.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            boot_time.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size != std::mem::size_of::<libc::timeval>() {
        return Err(vec![AppletError::new(APPLET, "reading boot time failed")]);
    }

    // SAFETY: `sysctlbyname` initialized `boot_time` on success.
    Ok(unsafe { boot_time.assume_init() }.tv_sec)
}

#[cfg(target_os = "macos")]
fn load_averages() -> Result<[f64; 3], Vec<AppletError>> {
    let mut loads = [0.0_f64; 3];
    // SAFETY: `loads` points to writable memory for three load samples.
    let result = unsafe { libc::getloadavg(loads.as_mut_ptr(), loads.len() as libc::c_int) };
    if result != loads.len() as libc::c_int {
        return Err(vec![AppletError::new(
            APPLET,
            "reading load averages failed",
        )]);
    }
    Ok(loads)
}

#[cfg(target_os = "macos")]
fn user_count() -> Result<usize, Vec<AppletError>> {
    let mut users = 0_usize;

    // SAFETY: utmpx iteration uses libc-managed global state. We reset it,
    // iterate until null, read the current entry, then close the database.
    unsafe {
        libc::setutxent();
        loop {
            let entry = libc::getutxent();
            if entry.is_null() {
                break;
            }
            if (*entry).ut_type == libc::USER_PROCESS {
                users += 1;
            }
        }
        libc::endutxent();
    }

    Ok(users)
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, format_snapshot, format_uptime, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(parse_args(&args(&["extra"])).is_err());
    }

    #[test]
    fn formats_short_uptime_in_minutes() {
        assert_eq!(format_uptime(59 * 60), "59 mins");
        assert_eq!(format_uptime(60), "1 min");
    }

    #[test]
    fn formats_hour_and_day_uptimes() {
        assert_eq!(format_uptime(61 * 60), "1:01");
        assert_eq!(format_uptime((24 * 60 + 1) * 60), "1 day, 0:01");
        assert_eq!(
            format_uptime((2 * 24 * 60 + 21 * 60 + 1) * 60),
            "2 days, 21:01"
        );
    }

    #[test]
    fn formats_snapshot_line() {
        let snapshot = Snapshot {
            now: 0,
            uptime_seconds: (2 * 24 * 60 + 21 * 60 + 1) * 60,
            users: 1,
            loads: [1.25, 2.50, 3.75],
        };
        let rendered = format_snapshot(&snapshot);
        assert!(rendered.contains("up 2 days, 21:01, 1 user, load averages: 1.25 2.50 3.75"));
    }
}
