use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "kill";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let mut list_signals = false;
    let mut signal: libc::c_int = libc::SIGTERM;
    let mut pids: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    pids.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-l" | "--list" => {
                list_signals = true;
            }
            "-s" | "--signal" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "s")]);
                }
                signal = parse_signal(APPLET, &args[i])?;
            }
            a if a.starts_with('-') && a.len() > 1 => {
                // -N or -SIGNAME or -NAME
                signal = parse_signal(APPLET, &a[1..])?;
            }
            _ => pids.push(arg),
        }
        i += 1;
    }

    let mut out = stdout();

    if list_signals {
        if pids.is_empty() {
            print_signal_list(&mut out)
                .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
        } else {
            for p in &pids {
                if let Ok(n) = p.parse::<libc::c_int>() {
                    let name = signal_name(n).ok_or_else(|| {
                        vec![AppletError::new(APPLET, format!("invalid signal number: {n}"))]
                    })?;
                    writeln!(out, "{name}")
                        .map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
                } else {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("invalid argument: '{p}'"),
                    )]);
                }
            }
        }
        return Ok(());
    }

    if pids.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let mut errors = Vec::new();
    for pid_str in &pids {
        match pid_str.parse::<libc::pid_t>() {
            Ok(pid) => {
                // SAFETY: kill is always safe to call; we check the return value.
                let ret = unsafe { libc::kill(pid, signal) };
                if ret != 0 {
                    let err = std::io::Error::last_os_error();
                    errors.push(AppletError::from_io(APPLET, "kill", Some(pid_str), err));
                }
            }
            Err(_) => {
                errors.push(AppletError::new(
                    APPLET,
                    format!("invalid pid: '{pid_str}'"),
                ));
            }
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn parse_signal(applet: &'static str, s: &str) -> Result<libc::c_int, Vec<AppletError>> {
    if let Ok(n) = s.parse::<libc::c_int>() {
        if n >= 0 {
            return Ok(n);
        }
    }
    let upper = s.to_ascii_uppercase();
    let name = upper.strip_prefix("SIG").unwrap_or(&upper);
    signal_number(name)
        .ok_or_else(|| vec![AppletError::new(applet, format!("invalid signal: '{s}'"))])
}

fn signal_number(name: &str) -> Option<libc::c_int> {
    Some(match name {
        "HUP" => libc::SIGHUP,
        "INT" => libc::SIGINT,
        "QUIT" => libc::SIGQUIT,
        "ILL" => libc::SIGILL,
        "TRAP" => libc::SIGTRAP,
        "ABRT" | "IOT" => libc::SIGABRT,
        "BUS" => libc::SIGBUS,
        "FPE" => libc::SIGFPE,
        "KILL" => libc::SIGKILL,
        "USR1" => libc::SIGUSR1,
        "SEGV" => libc::SIGSEGV,
        "USR2" => libc::SIGUSR2,
        "PIPE" => libc::SIGPIPE,
        "ALRM" => libc::SIGALRM,
        "TERM" => libc::SIGTERM,
        "CHLD" | "CLD" => libc::SIGCHLD,
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
        _ => return None,
    })
}

fn signal_name(n: libc::c_int) -> Option<&'static str> {
    Some(match n {
        n if n == libc::SIGHUP => "HUP",
        n if n == libc::SIGINT => "INT",
        n if n == libc::SIGQUIT => "QUIT",
        n if n == libc::SIGILL => "ILL",
        n if n == libc::SIGTRAP => "TRAP",
        n if n == libc::SIGABRT => "ABRT",
        n if n == libc::SIGBUS => "BUS",
        n if n == libc::SIGFPE => "FPE",
        n if n == libc::SIGKILL => "KILL",
        n if n == libc::SIGUSR1 => "USR1",
        n if n == libc::SIGSEGV => "SEGV",
        n if n == libc::SIGUSR2 => "USR2",
        n if n == libc::SIGPIPE => "PIPE",
        n if n == libc::SIGALRM => "ALRM",
        n if n == libc::SIGTERM => "TERM",
        n if n == libc::SIGCHLD => "CHLD",
        n if n == libc::SIGCONT => "CONT",
        n if n == libc::SIGSTOP => "STOP",
        n if n == libc::SIGTSTP => "TSTP",
        n if n == libc::SIGTTIN => "TTIN",
        n if n == libc::SIGTTOU => "TTOU",
        n if n == libc::SIGURG => "URG",
        n if n == libc::SIGXCPU => "XCPU",
        n if n == libc::SIGXFSZ => "XFSZ",
        n if n == libc::SIGVTALRM => "VTALRM",
        n if n == libc::SIGPROF => "PROF",
        n if n == libc::SIGWINCH => "WINCH",
        _ => return None,
    })
}

// Ordered list for -l output: (number, name)
const SIGNAL_LIST: &[(libc::c_int, &str)] = &[
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
];

fn print_signal_list(out: &mut dyn std::io::Write) -> std::io::Result<()> {
    for chunk in SIGNAL_LIST.chunks(4) {
        let mut line = String::new();
        for &(n, name) in chunk {
            line.push_str(&format!("{n:2}) SIG{name:<8}"));
        }
        writeln!(out, "{}", line.trim_end())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_signal, run, signal_name};

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_by_number() {
        assert_eq!(parse_signal("kill", "9").unwrap(), libc::SIGKILL);
    }

    #[test]
    fn parse_by_name() {
        assert_eq!(parse_signal("kill", "KILL").unwrap(), libc::SIGKILL);
    }

    #[test]
    fn parse_sig_prefix() {
        assert_eq!(parse_signal("kill", "SIGTERM").unwrap(), libc::SIGTERM);
    }

    #[test]
    fn parse_lowercase() {
        assert_eq!(parse_signal("kill", "term").unwrap(), libc::SIGTERM);
    }

    #[test]
    fn parse_invalid_fails() {
        assert!(parse_signal("kill", "BOGUS").is_err());
    }

    #[test]
    fn signal_name_roundtrip() {
        assert_eq!(signal_name(libc::SIGKILL), Some("KILL"));
        assert_eq!(signal_name(libc::SIGTERM), Some("TERM"));
        assert_eq!(signal_name(999), None);
    }

    #[test]
    fn list_flag_succeeds() {
        assert!(run(&args(&["-l"])).is_ok());
    }

    #[test]
    fn list_with_number_succeeds() {
        assert!(run(&args(&["-l", "9"])).is_ok());
    }

    #[test]
    fn missing_operand_fails() {
        assert!(run(&args(&[])).is_err());
    }

    #[test]
    fn invalid_pid_fails() {
        assert!(run(&args(&["notapid"])).is_err());
    }

    #[test]
    fn signal_to_self() {
        // Send signal 0 (existence check) to ourselves — should succeed.
        let pid = std::process::id().to_string();
        assert!(run(&args(&["-s", "0", &pid])).is_ok());
    }
}
