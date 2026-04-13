#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Duration;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Halt,
    Poweroff,
    Reboot,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    delay_secs: u64,
    no_sync: bool,
    force: bool,
    write_wtmp_only: bool,
}

pub fn main_halt(args: &[String]) -> i32 {
    finish(run(Action::Halt, args))
}

pub fn main_poweroff(args: &[String]) -> i32 {
    finish(run(Action::Poweroff, args))
}

pub fn main_reboot(args: &[String]) -> i32 {
    finish(run(Action::Reboot, args))
}

fn run(action: Action, args: &[String]) -> AppletResult {
    let applet = action.applet_name();
    let options = parse_args(applet, args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(action, options);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = (action, options);

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        applet,
        "unsupported on this platform",
    )])
}

fn parse_args(applet: &'static str, args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg() {
        match token {
            ArgToken::ShortFlags(flags) => {
                parse_short_flags(applet, flags, &mut cursor, &mut options)?
            }
            ArgToken::Operand(_) => {}
        }
    }

    Ok(options)
}

fn parse_short_flags<'a>(
    applet: &'static str,
    flags: &'a str,
    cursor: &mut ArgCursor<'a>,
    options: &mut Options,
) -> Result<(), Vec<AppletError>> {
    for (index, flag) in flags.char_indices() {
        match flag {
            'n' => options.no_sync = true,
            'f' => options.force = true,
            'w' => options.write_wtmp_only = true,
            'd' => {
                let attached = &flags[index + flag.len_utf8()..];
                let value = cursor.next_value_or_attached(attached, applet, "d")?;
                options.delay_secs = value.parse::<u64>().map_err(|_| {
                    vec![AppletError::new(
                        applet,
                        format!("invalid delay '{value}'"),
                    )]
                })?;
                break;
            }
            _ => return Err(vec![AppletError::invalid_option(applet, flag)]),
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn run_linux(action: Action, options: Options) -> AppletResult {
    if options.delay_secs > 0 {
        thread::sleep(Duration::from_secs(options.delay_secs));
    }

    if options.write_wtmp_only {
        return Ok(());
    }

    if !options.no_sync {
        // SAFETY: sync() has no failure mode.
        unsafe { libc::sync() };
    }

    let result = if options.force {
        force_shutdown(action)
    } else {
        request_init_shutdown(action)
    };

    result.map_err(|err| vec![AppletError::new(action.applet_name(), err)])
}

#[cfg(target_os = "linux")]
fn request_init_shutdown(action: Action) -> Result<(), String> {
    let signal = action.init_signal();
    // SAFETY: kill() is called with a well-defined PID and signal number.
    let rc = unsafe { libc::kill(1, signal) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("signaling init: {}", std::io::Error::last_os_error()))
    }
}

#[cfg(target_os = "linux")]
fn force_shutdown(action: Action) -> Result<(), String> {
    // SAFETY: reboot() is called with a valid Linux reboot command constant.
    let rc = unsafe { libc::reboot(action.reboot_cmd()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "requesting shutdown: {}",
            std::io::Error::last_os_error()
        ))
    }
}

impl Action {
    fn applet_name(self) -> &'static str {
        match self {
            Action::Halt => "halt",
            Action::Poweroff => "poweroff",
            Action::Reboot => "reboot",
        }
    }

    #[cfg(target_os = "linux")]
    fn init_signal(self) -> libc::c_int {
        match self {
            Action::Halt => libc::SIGUSR1,
            Action::Poweroff => libc::SIGUSR2,
            Action::Reboot => libc::SIGTERM,
        }
    }

    #[cfg(target_os = "linux")]
    fn reboot_cmd(self) -> libc::c_int {
        match self {
            Action::Halt => libc::LINUX_REBOOT_CMD_HALT,
            Action::Poweroff => libc::LINUX_REBOOT_CMD_POWER_OFF,
            Action::Reboot => libc::LINUX_REBOOT_CMD_RESTART,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};
    #[cfg(target_os = "linux")]
    use super::Action;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_delay_and_flags() {
        assert_eq!(
            parse_args("reboot", &args(&["-nfd5", "ignored"]))
                .expect("parse shutdown flags"),
            Options {
                delay_secs: 5,
                no_sync: true,
                force: true,
                write_wtmp_only: false,
            }
        );
    }

    #[test]
    fn parses_write_wtmp_flag() {
        assert_eq!(
            parse_args("halt", &args(&["-w"])).expect("parse halt -w"),
            Options {
                write_wtmp_only: true,
                ..Options::default()
            }
        );
    }

    #[test]
    fn missing_delay_argument_errors() {
        assert!(parse_args("poweroff", &args(&["-d"])).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn maps_init_signals() {
        assert_eq!(Action::Halt.init_signal(), libc::SIGUSR1);
        assert_eq!(Action::Poweroff.init_signal(), libc::SIGUSR2);
        assert_eq!(Action::Reboot.init_signal(), libc::SIGTERM);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn maps_reboot_commands() {
        assert_eq!(Action::Halt.reboot_cmd(), libc::LINUX_REBOOT_CMD_HALT);
        assert_eq!(
            Action::Poweroff.reboot_cmd(),
            libc::LINUX_REBOOT_CMD_POWER_OFF
        );
        assert_eq!(Action::Reboot.reboot_cmd(), libc::LINUX_REBOOT_CMD_RESTART);
    }
}
