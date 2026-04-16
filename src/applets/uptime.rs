use std::ffi::CStr;
use std::fs;
use std::io::Write;
use std::mem::MaybeUninit;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "uptime";
const PROC_LOADAVG: &str = "/proc/loadavg";
const PROC_UPTIME: &str = "/proc/uptime";
const UTMP_PATHS: &[&str] = &["/run/utmp", "/var/run/utmp"];

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    parse_args(args)?;
    let snapshot = collect_snapshot()?;
    let mut out = stdout();
    writeln!(out, "{}", format_snapshot(&snapshot))
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
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
    let c_seconds = seconds;
    let mut tm = MaybeUninit::<libc::tm>::uninit();
    let format = b"%H:%M\0";
    let mut buffer = [0 as libc::c_char; 16];

    // SAFETY: `c_seconds`, `tm`, `buffer`, and `format` point to valid memory
    // for the duration of these libc calls, and `format` is NUL-terminated.
    let written = unsafe {
        if libc::localtime_r(&c_seconds, tm.as_mut_ptr()).is_null() {
            return Err(AppletError::new(APPLET, "failed to format time"));
        }
        libc::strftime(
            buffer.as_mut_ptr(),
            buffer.len(),
            format.as_ptr().cast(),
            tm.as_ptr(),
        )
    };
    if written == 0 {
        return Err(AppletError::new(APPLET, "failed to format time"));
    }
    // SAFETY: `strftime` writes a NUL-terminated string when it succeeds.
    Ok(unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn collect_snapshot() -> Result<Snapshot, Vec<AppletError>> {
    let now = current_time()?;
    let uptime_seconds = read_uptime_seconds()?;
    let users = user_count()?;
    let loads = read_load_averages()?;

    Ok(Snapshot {
        now,
        uptime_seconds,
        users,
        loads,
    })
}

fn current_time() -> Result<i64, Vec<AppletError>> {
    // SAFETY: null tells libc to return the current time without writing it.
    let result = unsafe { libc::time(std::ptr::null_mut()) };
    if result == -1 {
        return Err(vec![AppletError::new(
            APPLET,
            "reading current time failed",
        )]);
    }
    Ok(result as i64)
}

fn read_uptime_seconds() -> Result<u64, Vec<AppletError>> {
    let uptime = fs::read_to_string(PROC_UPTIME).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(PROC_UPTIME),
            err,
        )]
    })?;
    let seconds = uptime
        .split_whitespace()
        .next()
        .ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("invalid {PROC_UPTIME} contents"),
            )]
        })?
        .parse::<f64>()
        .map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid {PROC_UPTIME} contents"),
            )]
        })?;
    Ok(seconds as u64)
}

fn read_load_averages() -> Result<[f64; 3], Vec<AppletError>> {
    let loadavg = fs::read_to_string(PROC_LOADAVG).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(PROC_LOADAVG),
            err,
        )]
    })?;
    let mut parts = loadavg.split_whitespace();
    let mut loads = [0.0_f64; 3];
    for load in &mut loads {
        *load = parts
            .next()
            .ok_or_else(|| {
                vec![AppletError::new(
                    APPLET,
                    format!("invalid {PROC_LOADAVG} contents"),
                )]
            })?
            .parse::<f64>()
            .map_err(|_| {
                vec![AppletError::new(
                    APPLET,
                    format!("invalid {PROC_LOADAVG} contents"),
                )]
            })?;
    }
    Ok(loads)
}

fn user_count() -> Result<usize, Vec<AppletError>> {
    for path in UTMP_PATHS {
        match fs::read(path) {
            Ok(bytes) => return Ok(count_utmp_users(&bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(path),
                    err,
                )]);
            }
        }
    }
    Ok(0)
}

fn count_utmp_users(bytes: &[u8]) -> usize {
    let size = std::mem::size_of::<libc::utmpx>();
    if size == 0 {
        return 0;
    }

    bytes
        .chunks_exact(size)
        .filter(|chunk| utmp_entry_is_user_process(chunk))
        .count()
}

fn utmp_entry_is_user_process(chunk: &[u8]) -> bool {
    if chunk.len() != std::mem::size_of::<libc::utmpx>() {
        return false;
    }

    // SAFETY: `chunk` is exactly one `utmpx` record wide and may be
    // unaligned, so `read_unaligned` is the correct way to inspect it.
    let entry = unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast::<libc::utmpx>()) };
    entry.ut_type == libc::USER_PROCESS
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, format_snapshot, format_uptime, parse_args};
    use super::count_utmp_users;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
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

    #[test]
    fn counts_user_process_entries_in_utmp() {
        let mut user = unsafe { std::mem::zeroed::<libc::utmpx>() };
        user.ut_type = libc::USER_PROCESS;
        user.ut_user[0] = b'a' as libc::c_char;

        let mut dead = unsafe { std::mem::zeroed::<libc::utmpx>() };
        dead.ut_type = libc::DEAD_PROCESS;

        let mut bytes = Vec::new();
        // SAFETY: both values are fully initialized plain-old-data records.
        bytes.extend_from_slice(unsafe { any_as_bytes(&user) });
        // SAFETY: both values are fully initialized plain-old-data records.
        bytes.extend_from_slice(unsafe { any_as_bytes(&dead) });

        assert_eq!(count_utmp_users(&bytes), 1);
    }

    unsafe fn any_as_bytes<T>(value: &T) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
        }
    }
}
