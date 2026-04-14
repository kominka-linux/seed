use std::ffi::CString;
use std::fs;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "rmmod";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all_unused: bool,
    force: bool,
    wait: bool,
    modules: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    run_linux(options)
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'a' => options.all_unused = true,
                    'f' => options.force = true,
                    'w' => options.wait = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            continue;
        }
        options.modules.push(arg.clone());
    }

    if !options.all_unused && options.modules.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok(options)
}

fn run_linux(options: Options) -> AppletResult {
    let flags = delete_module_flags(&options);
    let modules = if options.all_unused {
        if options.modules.is_empty() {
            unused_modules()?
        } else {
            options.modules.clone()
        }
    } else {
        options.modules.clone()
    };

    let mut errors = Vec::new();
    for module in modules {
        let context = if options.all_unused && options.modules.is_empty() {
            None
        } else {
            Some(module.as_str())
        };
        if let Err(err) = delete_module(&module, flags, context) {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn delete_module_flags(options: &Options) -> libc::c_int {
    let mut flags = if options.wait { 0 } else { libc::O_NONBLOCK };
    if options.force {
        flags |= libc::O_TRUNC;
    }
    flags
}

fn delete_module(module: &str, flags: libc::c_int, name: Option<&str>) -> Result<(), AppletError> {
    let module = CString::new(module)
        .map_err(|_| AppletError::new(APPLET, "module name contains NUL byte"))?;
    // SAFETY: `module` is a valid NUL-terminated string, and `flags` only uses
    // documented delete_module flag bits.
    let rc = unsafe { libc::syscall(libc::SYS_delete_module, module.as_ptr(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        let message = match name {
            Some(name) => format!("can't unload module '{name}': {err}"),
            None => format!("rmmod: {err}"),
        };
        Err(AppletError::new(APPLET, message))
    }
}

fn unused_modules() -> Result<Vec<String>, Vec<AppletError>> {
    let modules = fs::read_to_string("/proc/modules")
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some("/proc/modules"), err)])?;

    Ok(modules
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            let _size = fields.next()?;
            let use_count = fields.next()?.parse::<u64>().ok()?;
            if use_count == 0 {
                Some(name.to_owned())
            } else {
                None
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};
    use super::delete_module_flags;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_modules() {
        assert_eq!(
            parse_args(&args(&["-wf", "mod_a", "mod_b"])).expect("parse rmmod"),
            Options {
                all_unused: false,
                force: true,
                wait: true,
                modules: vec![String::from("mod_a"), String::from("mod_b")],
            }
        );
    }

    #[test]
    fn requires_operand_without_a() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&args(&["-a"])).is_ok());
    }

    #[test]
    fn maps_delete_module_flags() {
        let options = Options {
            all_unused: false,
            force: true,
            wait: false,
            modules: vec![String::from("mod")],
        };
        assert_eq!(delete_module_flags(&options), libc::O_NONBLOCK | libc::O_TRUNC);
    }
}
