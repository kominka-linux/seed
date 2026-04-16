use std::io::Write;

use std::fs;

use crate::common::applet::finish;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "sysctl";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    values_only: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let (options, names) = parse_args(&args)?;
    let mut out = stdout();

    for name in names {
        let value = read_value(&name)?;
        if options.values_only {
            writeln!(out, "{value}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        } else {
            writeln!(out, "{name}: {value}")
                .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut names = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg == "-n" {
            options.values_only = true;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            let flag = arg.chars().nth(1).expect("short option has flag");
            return Err(vec![AppletError::invalid_option(APPLET, flag)]);
        }
        names.push(arg.clone());
    }

    if names.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((options, names))
}

fn read_value(name: &str) -> Result<String, Vec<AppletError>> {
    read_linux_value(name)
}

fn read_linux_value(name: &str) -> Result<String, Vec<AppletError>> {
    let path = linux_sysctl_path(name)?;
    let value = fs::read_to_string(&path)
        .map_err(|_| vec![AppletError::new(APPLET, format!("unknown key '{name}'"))])?;
    Ok(value.trim_end().to_string())
}

fn linux_sysctl_path(name: &str) -> Result<String, Vec<AppletError>> {
    if name.is_empty()
        || name
            .split('.')
            .any(|part| part.is_empty() || part.contains('/'))
    {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid name '{name}'"),
        )]);
    }
    Ok(format!("/proc/sys/{}", name.replace('.', "/")))
}

#[cfg(test)]
mod tests {
    use super::linux_sysctl_path;
    use super::{Options, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_n_and_names() {
        let (options, names) =
            parse_args(&args(&["-n", "kernel.ostype", "kernel.pid_max"])).expect("parse sysctl");
        assert_eq!(options, Options { values_only: true });
        assert_eq!(
            names,
            vec![
                String::from("kernel.ostype"),
                String::from("kernel.pid_max")
            ]
        );
    }

    #[test]
    fn maps_linux_keys_to_proc_sys_paths() {
        assert_eq!(
            linux_sysctl_path("kernel.ostype").expect("linux key"),
            "/proc/sys/kernel/ostype"
        );
        assert!(linux_sysctl_path("kernel/ostype").is_err());
    }
}
