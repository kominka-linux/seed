use std::ffi::OsString;
use std::io::Write;

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::Parser;
use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::list_processes;

const APPLET: &str = "killall5";
const SIG_STKFLT: libc::c_int = 16;
const SIG_POLL: libc::c_int = 29;
const SIG_PWR: libc::c_int = 30;
const SIG_SYS: libc::c_int = 31;
const SIG_RTMIN_LINUX: libc::c_int = 35;
const SIG_RTMAX_LINUX: libc::c_int = 64;

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    let options = parse_args(args)?;
    if options.list_signals {
        return print_signal_list()
            .map(|()| 0)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)]);
    }
    run_linux(options).map(|()| 0)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    list_signals: bool,
    signal: libc::c_int,
    omit_pids: Vec<i32>,
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut list_signals = false;
    let mut signal = libc::SIGTERM;
    let mut omit_pids = Vec::new();
    let mut parser = Parser::new(APPLET, args);
    let mut args = parser.raw_args()?;

    while let Some(arg) = args.next() {
        let arg = to_string(APPLET, arg)?;
        match arg.as_str() {
            "-l" => list_signals = true,
            "-o" => {
                let Some(value) = args.next() else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "o")]);
                };
                omit_pids.push(parse_pid(&to_string(APPLET, value)?)?);
            }
            a if a.starts_with('-') && a.len() > 1 => {
                signal = parse_signal(&a[1..])?;
            }
            _ => {}
        }
    }

    Ok(Options {
        list_signals,
        signal,
        omit_pids,
    })
}

fn parse_pid(value: &str) -> Result<i32, Vec<AppletError>> {
    value
        .parse::<i32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid number '{value}'"))])
}

fn to_string(applet: &'static str, value: OsString) -> Result<String, Vec<AppletError>> {
    value.into_string().map_err(|value| {
        vec![AppletError::new(
            applet,
            format!("argument is invalid unicode: {:?}", value),
        )]
    })
}

fn parse_signal(value: &str) -> Result<libc::c_int, Vec<AppletError>> {
    if let Ok(signal) = value.parse::<libc::c_int>()
        && signal >= 0
    {
        return Ok(signal);
    }

    let upper = value.to_ascii_uppercase();
    let name = upper.strip_prefix("SIG").unwrap_or(&upper);
    signal_number(name)
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("bad signal name '{value}'"))])
}

fn signal_number(name: &str) -> Option<libc::c_int> {
    Some(match name {
        "HUP" => libc::SIGHUP,
        "INT" => libc::SIGINT,
        "QUIT" => libc::SIGQUIT,
        "ILL" => libc::SIGILL,
        "TRAP" => libc::SIGTRAP,
        "ABRT" => libc::SIGABRT,
        "BUS" => libc::SIGBUS,
        "FPE" => libc::SIGFPE,
        "KILL" => libc::SIGKILL,
        "USR1" => libc::SIGUSR1,
        "SEGV" => libc::SIGSEGV,
        "USR2" => libc::SIGUSR2,
        "PIPE" => libc::SIGPIPE,
        "ALRM" => libc::SIGALRM,
        "TERM" => libc::SIGTERM,
        "STKFLT" => SIG_STKFLT,
        "CHLD" => libc::SIGCHLD,
        "CONT" => libc::SIGCONT,
        "STOP" => libc::SIGSTOP,
        "TSTP" => libc::SIGTSTP,
        "TTIN" => libc::SIGTTIN,
        "TTOU" => libc::SIGTTOU,
        "URG" => libc::SIGURG,
        "XCPU" => libc::SIGXCPU,
        "XFSZ" => libc::SIGXFSZ,
        "VTALRM" => libc::SIGVTALRM,
        "PROF" => libc::SIGPROF,
        "WINCH" => libc::SIGWINCH,
        "POLL" => SIG_POLL,
        "PWR" => SIG_PWR,
        "SYS" => SIG_SYS,
        "RTMIN" => SIG_RTMIN_LINUX,
        "RTMAX" => SIG_RTMAX_LINUX,
        _ => return None,
    })
}

fn print_signal_list() -> std::io::Result<()> {
    let mut out = stdout();
    for (number, name) in signal_list() {
        writeln!(out, "{number:2}) {name}")?;
    }
    out.flush()
}

fn signal_list() -> Vec<(libc::c_int, &'static str)> {
    vec![
        (1, "HUP"),
        (2, "INT"),
        (3, "QUIT"),
        (4, "ILL"),
        (5, "TRAP"),
        (6, "ABRT"),
        (7, "BUS"),
        (8, "FPE"),
        (9, "KILL"),
        (10, "USR1"),
        (11, "SEGV"),
        (12, "USR2"),
        (13, "PIPE"),
        (14, "ALRM"),
        (15, "TERM"),
        (SIG_STKFLT, "STKFLT"),
        (17, "CHLD"),
        (18, "CONT"),
        (19, "STOP"),
        (20, "TSTP"),
        (21, "TTIN"),
        (22, "TTOU"),
        (23, "URG"),
        (24, "XCPU"),
        (25, "XFSZ"),
        (26, "VTALRM"),
        (27, "PROF"),
        (28, "WINCH"),
        (SIG_POLL, "POLL"),
        (SIG_PWR, "PWR"),
        (SIG_SYS, "SYS"),
        (SIG_RTMIN_LINUX, "RTMIN"),
        (SIG_RTMAX_LINUX, "RTMAX"),
    ]
}

fn run_linux(options: Options) -> Result<(), Vec<AppletError>> {
    let current_sid = current_session_id()?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let self_pid = std::process::id() as i32;
    let mut errors = Vec::new();

    for process in processes {
        if process.pid == self_pid || options.omit_pids.contains(&process.pid) {
            continue;
        }
        let Ok(process_sid) = session_id_of(process.pid) else {
            continue;
        };
        if process_sid == current_sid {
            continue;
        }

        // SAFETY: `process.pid` came from the kernel process table and
        // `options.signal` is either parsed from a valid number or mapped from
        // a known signal name.
        let rc = unsafe { libc::kill(process.pid, options.signal) };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                errors.push(AppletError::from_io(APPLET, "signaling", None, error));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn current_session_id() -> Result<i32, Vec<AppletError>> {
    session_id_of(0).map_err(|err| vec![AppletError::new(APPLET, format!("getsid: {err}"))])
}

fn session_id_of(pid: i32) -> Result<i32, std::io::Error> {
    // SAFETY: `getsid` is called with a valid pid or 0 for the current process.
    let sid = unsafe { libc::getsid(pid) };
    if sid >= 0 {
        Ok(sid)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_args, parse_signal};

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_list_and_omit() {
        let options = parse_args(&args(&["-l", "-o", "123"])).expect("parse killall5");
        assert!(options.list_signals);
        assert_eq!(options.omit_pids, vec![123]);
    }

    #[test]
    fn parses_named_signal() {
        assert_eq!(parse_signal("TERM").expect("signal"), libc::SIGTERM);
    }

    #[test]
    fn rejects_bad_signal() {
        assert!(parse_signal("z").is_err());
    }
}
