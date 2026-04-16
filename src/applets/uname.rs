use std::ffi::CStr;
use std::mem::MaybeUninit;

use crate::common::applet::fail;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET: &str = "uname";

#[derive(Clone, Copy, Debug, Default)]
struct Options {
    all: bool,
    machine: bool,
    nodename: bool,
    release: bool,
    sysname: bool,
    processor: bool,
    version: bool,
    hardware_platform: bool,
    operating_system: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    match run(args) {
        Ok(output) => {
            println!("{output}");
            0
        }
        Err(errors) => fail(errors, 1),
    }
}

fn run(args: &[std::ffi::OsString]) -> Result<String, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    let mut options = parse_args(&args)?;
    if options.all {
        options.machine = true;
        options.nodename = true;
        options.release = true;
        options.sysname = true;
        options.processor = true;
        options.version = true;
        options.hardware_platform = true;
        options.operating_system = true;
    }
    if !(options.machine
        || options.nodename
        || options.release
        || options.sysname
        || options.processor
        || options.version
        || options.hardware_platform
        || options.operating_system)
    {
        options.sysname = true;
    }

    let uts = utsname()?;
    let machine = field_to_string(&uts.machine);
    let mut fields = Vec::new();
    if options.sysname {
        fields.push(field_to_string(&uts.sysname));
    }
    if options.nodename {
        fields.push(field_to_string(&uts.nodename));
    }
    if options.release {
        fields.push(field_to_string(&uts.release));
    }
    if options.version {
        fields.push(field_to_string(&uts.version));
    }
    if options.machine {
        fields.push(machine.clone());
    }
    if options.processor {
        fields.push(machine.clone());
    }
    if options.hardware_platform {
        fields.push(machine.clone());
    }
    if options.operating_system {
        fields.push(operating_system_name().to_string());
    }

    Ok(fields.join(" "))
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    for arg in args {
        if !arg.starts_with('-') || arg == "-" {
            return Err(vec![AppletError::new(
                APPLET,
                format!("unexpected operand '{arg}'"),
            )]);
        }
        for flag in arg[1..].chars() {
            match flag {
                'a' => options.all = true,
                'm' => options.machine = true,
                'n' => options.nodename = true,
                'r' => options.release = true,
                's' => options.sysname = true,
                'p' => options.processor = true,
                'v' => options.version = true,
                'i' => options.hardware_platform = true,
                'o' => options.operating_system = true,
                _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            }
        }
    }
    Ok(options)
}

fn utsname() -> Result<libc::utsname, Vec<AppletError>> {
    let mut uts = MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: `uname` writes a fully initialized `utsname` into the provided pointer.
    let status = unsafe { libc::uname(uts.as_mut_ptr()) };
    if status == 0 {
        // SAFETY: `uname` initialized `uts` on success.
        Ok(unsafe { uts.assume_init() })
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("uname failed: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn field_to_string(field: &[libc::c_char]) -> String {
    // SAFETY: `utsname` fields are NUL-terminated C strings provided by libc.
    unsafe { CStr::from_ptr(field.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

fn operating_system_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        std::env::consts::OS
    }
    #[cfg(not(target_os = "macos"))]
    {
        "GNU/Linux"
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    #[test]
    fn defaults_to_sysname() {
        let options = parse_args(&[]).expect("parse uname args");
        assert!(!options.sysname);
    }

    #[test]
    fn parses_machine_flag() {
        let options = parse_args(&["-m".to_string()]).expect("parse uname args");
        assert!(options.machine);
    }
}
