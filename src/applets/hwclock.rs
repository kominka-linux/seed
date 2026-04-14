#![allow(deprecated)]
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]

#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::os::fd::RawFd;
#[cfg(target_os = "linux")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::applet::finish;
use crate::common::error::AppletError;

const APPLET: &str = "hwclock";
const RTC_RD_TIME: libc::c_ulong = 0x8024_7009;
const RTC_SET_TIME: libc::c_ulong = 0x4024_700a;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    device: Option<String>,
    utc: Option<bool>,
    show: bool,
    hctosys: bool,
    systohc: bool,
    systz: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct LinuxRtcTime {
    tm_sec: libc::c_int,
    tm_min: libc::c_int,
    tm_hour: libc::c_int,
    tm_mday: libc::c_int,
    tm_mon: libc::c_int,
    tm_year: libc::c_int,
    tm_wday: libc::c_int,
    tm_yday: libc::c_int,
    tm_isdst: libc::c_int,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct LinuxTimezone {
    tz_minuteswest: libc::c_int,
    tz_dsttime: libc::c_int,
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn tzset();
    static mut timezone: libc::c_long;
    #[link_name = "settimeofday"]
    fn settimeofday_sys(tv: *const libc::timeval, tz: *const LinuxTimezone) -> libc::c_int;
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(&options);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = options;

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options {
        show: true,
        ..Options::default()
    };
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        match arg.as_str() {
            "-u" => options.utc = Some(true),
            "-l" => options.utc = Some(false),
            "-r" | "--show" => options.show = true,
            "-s" | "--hctosys" => {
                options.hctosys = true;
                options.show = false;
            }
            "-w" => {
                options.systohc = true;
                options.show = false;
            }
            "--systz" => {
                options.systz = true;
                options.show = false;
            }
            "-f" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "f")]);
                };
                index += 1;
                options.device = Some(value.clone());
            }
            value if value.starts_with("--param-") => {
                return Err(vec![AppletError::new(
                    APPLET,
                    "RTC parameter access is not supported",
                )]);
            }
            _ if arg.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    arg.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }

    let action_count =
        usize::from(options.show) + usize::from(options.hctosys) + usize::from(options.systohc)
            + usize::from(options.systz);
    if action_count > 1 {
        return Err(vec![AppletError::new(APPLET, "conflicting actions specified")]);
    }

    Ok(options)
}

#[cfg(target_os = "linux")]
fn run_linux(options: &Options) -> Result<(), Vec<AppletError>> {
    let utc = options.utc.unwrap_or_else(adjtime_is_utc);
    match () {
        _ if options.hctosys => set_system_time_from_rtc(options.device.as_deref(), utc),
        _ if options.systohc => set_rtc_from_system_time(options.device.as_deref(), utc),
        _ if options.systz => set_kernel_timezone(utc),
        _ => show_clock(options.device.as_deref(), utc),
    }
}

#[cfg(target_os = "linux")]
fn show_clock(device: Option<&str>, utc: bool) -> Result<(), Vec<AppletError>> {
    let time = read_rtc_time(device, utc)?;
    println!("{}  0.000000 seconds", format_ctime(time));
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_system_time_from_rtc(device: Option<&str>, utc: bool) -> Result<(), Vec<AppletError>> {
    let seconds = read_rtc_time(device, utc)?;
    let tv = libc::timeval {
        tv_sec: seconds,
        tv_usec: 0,
    };
    let tz = system_timezone_struct(utc)?;
    // SAFETY: `tv` and `tz` are valid pointers for the duration of the call.
    let rc = unsafe { settimeofday_sys(&tv, &tz) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("settimeofday: {}", std::io::Error::last_os_error()),
        )])
    }
}

#[cfg(target_os = "linux")]
fn set_rtc_from_system_time(device: Option<&str>, utc: bool) -> Result<(), Vec<AppletError>> {
    let fd = open_rtc(device, libc::O_RDWR)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as libc::time_t;
    let tm = time_to_tm(now, utc)?;
    let rtc = tm_to_rtc(tm);
    // SAFETY: `rtc` points to a valid Linux RTC time structure and `fd` is a valid open RTC descriptor.
    let rc = unsafe { libc::ioctl(fd, RTC_SET_TIME as _, &rtc) };
    close_fd(fd);
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("RTC_SET_TIME: {}", std::io::Error::last_os_error()),
        )])
    }
}

#[cfg(target_os = "linux")]
fn set_kernel_timezone(utc: bool) -> Result<(), Vec<AppletError>> {
    // SAFETY: `tzset` updates libc global timezone state.
    unsafe { tzset() };
    let tz = system_timezone_struct(utc)?;
    let mut tv = libc::timeval {
        tv_sec: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as libc::time_t,
        tv_usec: 0,
    };
    if !utc {
        tv.tv_sec += i64::from(tz.tz_minuteswest) * 60;
    }
    // SAFETY: `tv` and `tz` are valid pointers for the duration of the call.
    let rc = unsafe { settimeofday_sys(&tv, &tz) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("settimeofday: {}", std::io::Error::last_os_error()),
        )])
    }
}

#[cfg(target_os = "linux")]
fn read_rtc_time(device: Option<&str>, utc: bool) -> Result<libc::time_t, Vec<AppletError>> {
    let fd = open_rtc(device, libc::O_RDONLY)?;
    let mut rtc = LinuxRtcTime::default();
    // SAFETY: `rtc` points to writable memory and `fd` is a valid open RTC descriptor.
    let rc = unsafe { libc::ioctl(fd, RTC_RD_TIME as _, &mut rtc) };
    let err = std::io::Error::last_os_error();
    close_fd(fd);
    if rc != 0 {
        return Err(vec![AppletError::new(APPLET, format!("RTC_RD_TIME: {err}"))]);
    }
    rtc_to_time(rtc, utc).map_err(|error| vec![error])
}

#[cfg(target_os = "linux")]
fn open_rtc(device: Option<&str>, flags: libc::c_int) -> Result<RawFd, Vec<AppletError>> {
    let candidates = if let Some(device) = device {
        vec![device.to_string()]
    } else {
        vec![
            String::from("/dev/rtc"),
            String::from("/dev/rtc0"),
            String::from("/dev/misc/rtc"),
        ]
    };

    let mut last_error = None;
    for path in candidates {
        let c_path = CString::new(path.as_str())
            .map_err(|_| vec![AppletError::new(APPLET, "RTC path contains NUL byte")])?;
        // SAFETY: `c_path` is a valid NUL-terminated path string.
        let fd = unsafe { libc::open(c_path.as_ptr(), flags) };
        if fd >= 0 {
            return Ok(fd);
        }
        last_error = Some((path, std::io::Error::last_os_error()));
    }

    let (path, err) = last_error.unwrap_or_else(|| (String::from("/dev/rtc"), std::io::Error::from_raw_os_error(libc::ENOENT)));
    Err(vec![AppletError::from_io(APPLET, "opening", Some(&path), err)])
}

#[cfg(target_os = "linux")]
fn close_fd(fd: RawFd) {
    // SAFETY: `fd` is an open file descriptor returned from `open`.
    unsafe {
        libc::close(fd);
    }
}

#[cfg(target_os = "linux")]
fn rtc_to_time(rtc: LinuxRtcTime, utc: bool) -> Result<libc::time_t, AppletError> {
    let mut tm = libc::tm {
        tm_sec: rtc.tm_sec,
        tm_min: rtc.tm_min,
        tm_hour: rtc.tm_hour,
        tm_mday: rtc.tm_mday,
        tm_mon: rtc.tm_mon,
        tm_year: rtc.tm_year,
        tm_wday: rtc.tm_wday,
        tm_yday: rtc.tm_yday,
        tm_isdst: -1,
        #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "emscripten",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "haiku",
            target_os = "hermit",
            target_os = "illumos",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "solaris"
        ))]
        tm_gmtoff: 0,
        #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "emscripten",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "haiku",
            target_os = "illumos",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "solaris"
        ))]
        tm_zone: std::ptr::null(),
    };
    tm_to_time(&mut tm, utc)
}

#[cfg(target_os = "linux")]
fn tm_to_rtc(tm: libc::tm) -> LinuxRtcTime {
    LinuxRtcTime {
        tm_sec: tm.tm_sec,
        tm_min: tm.tm_min,
        tm_hour: tm.tm_hour,
        tm_mday: tm.tm_mday,
        tm_mon: tm.tm_mon,
        tm_year: tm.tm_year,
        tm_wday: tm.tm_wday,
        tm_yday: tm.tm_yday,
        tm_isdst: 0,
    }
}

#[cfg(target_os = "linux")]
fn tm_to_time(tm: &mut libc::tm, utc: bool) -> Result<libc::time_t, AppletError> {
    if utc {
        unsafe {
            let old = env::var_os("TZ");
            env::set_var("TZ", "UTC0");
            tzset();
            let result = libc::mktime(tm);
            match old {
                Some(value) => env::set_var("TZ", value),
                None => env::remove_var("TZ"),
            }
            tzset();
            if result >= 0 {
                Ok(result)
            } else {
                Err(AppletError::new(APPLET, "converting RTC time failed"))
            }
        }
    } else {
        // SAFETY: `tm` points to a valid time structure for libc conversion.
        let result = unsafe { libc::mktime(tm) };
        if result >= 0 {
            Ok(result)
        } else {
            Err(AppletError::new(APPLET, "converting RTC time failed"))
        }
    }
}

#[cfg(target_os = "linux")]
fn time_to_tm(value: libc::time_t, utc: bool) -> Result<libc::tm, Vec<AppletError>> {
    let mut tm = zeroed_tm();
    let rc = if utc {
        // SAFETY: pointers remain valid for the duration of the call.
        unsafe { libc::gmtime_r(&value, &mut tm) }
    } else {
        // SAFETY: pointers remain valid for the duration of the call.
        unsafe { libc::localtime_r(&value, &mut tm) }
    };
    if rc.is_null() {
        Err(vec![AppletError::new(APPLET, "converting system time failed")])
    } else {
        Ok(tm)
    }
}

#[cfg(target_os = "linux")]
fn zeroed_tm() -> libc::tm {
    libc::tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "emscripten",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "haiku",
            target_os = "hermit",
            target_os = "illumos",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "solaris"
        ))]
        tm_gmtoff: 0,
        #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "emscripten",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "haiku",
            target_os = "illumos",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "solaris"
        ))]
        tm_zone: std::ptr::null(),
    }
}

#[cfg(target_os = "linux")]
fn system_timezone_struct(utc: bool) -> Result<LinuxTimezone, Vec<AppletError>> {
    let local = time_to_tm(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as libc::time_t,
        false,
    )?;
    // SAFETY: `tzset` refreshes libc timezone globals before we read `timezone`.
    unsafe { tzset() };
    let mut minuteswest = unsafe { (timezone / 60) as libc::c_int };
    if local.tm_isdst > 0 {
        minuteswest -= 60;
    }
    Ok(LinuxTimezone {
        tz_minuteswest: if utc { 0 } else { minuteswest },
        tz_dsttime: 0,
    })
}

#[cfg(target_os = "linux")]
fn adjtime_is_utc() -> bool {
    let path = env::var_os("SEED_ADJTIME")
        .map(Into::into)
        .unwrap_or_else(|| PathBuf::from("/etc/adjtime"));
    fs::read_to_string(path)
        .ok()
        .is_some_and(|text| text.lines().any(|line| line.trim() == "UTC"))
}

#[cfg(target_os = "linux")]
fn format_ctime(value: libc::time_t) -> String {
    let mut tm = zeroed_tm();
    // SAFETY: pointers remain valid for the duration of the call.
    let ptr = unsafe { libc::localtime_r(&value, &mut tm) };
    if ptr.is_null() {
        return String::from("Thu Jan  1 00:00:00 1970");
    }
    let mut buffer = [0_u8; 64];
    // SAFETY: buffer is writable and large enough for the requested format.
    let written = unsafe {
        libc::strftime(
            buffer.as_mut_ptr() as *mut libc::c_char,
            buffer.len(),
            c"%a %b %e %H:%M:%S %Y".as_ptr(),
            &tm,
        )
    };
    String::from_utf8_lossy(&buffer[..written]).into_owned()
}

#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};
    #[cfg(target_os = "linux")]
    use super::adjtime_is_utc;
    #[cfg(target_os = "linux")]
    use crate::common::test_env;
    #[cfg(target_os = "linux")]
    use std::fs;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_hwclock_actions() {
        assert_eq!(
            parse_args(&args(&["-u", "-f", "/dev/rtc1", "-w"])).unwrap(),
            Options {
                device: Some("/dev/rtc1".to_string()),
                utc: Some(true),
                show: false,
                systohc: true,
                ..Options::default()
            }
        );
    }

    #[test]
    fn parses_hctosys_long_option() {
        assert_eq!(
            parse_args(&args(&["--hctosys"])).unwrap(),
            Options {
                show: false,
                hctosys: true,
                ..Options::default()
            }
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_adjtime_utc_marker() {
        let _guard = test_env::lock();
        let path = std::env::temp_dir().join(format!("seed-adjtime-{}", std::process::id()));
        fs::write(&path, "0.0 0 0\n0\nUTC\n").unwrap();
        unsafe {
            std::env::set_var("SEED_ADJTIME", &path);
        }
        assert!(adjtime_is_utc());
        unsafe {
            std::env::remove_var("SEED_ADJTIME");
        }
        let _ = fs::remove_file(path);
    }
}
