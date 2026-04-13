use std::ffi::CString;
use std::io::Write;
use std::mem::MaybeUninit;

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "free";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Unit {
    Bytes,
    KiB,
    MiB,
    GiB,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Options {
    unit: Unit,
}

#[derive(Clone, Copy, Debug)]
struct Stats {
    mem_total: u64,
    mem_used: u64,
    mem_free: u64,
    swap_total: u64,
    swap_used: u64,
    swap_free: u64,
}

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
    let options = parse_args(args)?;
    let stats = read_stats()?;
    let mut out = stdout();
    writeln!(out, "{:>12} {:>12} {:>12}", "total", "used", "free")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    writeln!(
        out,
        "{:<5} {:>12} {:>12} {:>12}",
        "Mem:",
        scale(stats.mem_total, options.unit),
        scale(stats.mem_used, options.unit),
        scale(stats.mem_free, options.unit)
    )
    .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    writeln!(
        out,
        "{:<5} {:>12} {:>12} {:>12}",
        "Swap:",
        scale(stats.swap_total, options.unit),
        scale(stats.swap_used, options.unit),
        scale(stats.swap_free, options.unit)
    )
    .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut unit = Unit::KiB;
    for arg in args {
        if !arg.starts_with('-') || arg.len() != 2 {
            return Err(vec![AppletError::new(APPLET, "extra operand")]);
        }
        unit = match arg.as_bytes()[1] {
            b'b' => Unit::Bytes,
            b'k' => Unit::KiB,
            b'm' => Unit::MiB,
            b'g' => Unit::GiB,
            flag => return Err(vec![AppletError::invalid_option(APPLET, flag as char)]),
        };
    }
    Ok(Options { unit })
}

fn scale(value: u64, unit: Unit) -> u64 {
    match unit {
        Unit::Bytes => value,
        Unit::KiB => value / 1024,
        Unit::MiB => value / (1024 * 1024),
        Unit::GiB => value / (1024 * 1024 * 1024),
    }
}

#[cfg(target_os = "macos")]
fn read_stats() -> Result<Stats, Vec<AppletError>> {
    let mem_total = read_sysctl_u64("hw.memsize")?;
    let page_size = read_sysctl_u64("hw.pagesize")?;

    let mut vm = MaybeUninit::<libc::vm_statistics64>::uninit();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `vm` points to writable storage for `host_statistics64`, and
    // `count` is initialized to the documented structure count.
    #[allow(deprecated)]
    let rc = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            vm.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if rc != libc::KERN_SUCCESS {
        return Err(vec![AppletError::new(
            APPLET,
            "reading virtual memory statistics failed",
        )]);
    }
    // SAFETY: `host_statistics64` initialized `vm` on success.
    let vm = unsafe { vm.assume_init() };
    let mem_free = (u64::from(vm.free_count) + u64::from(vm.speculative_count)) * page_size;
    let mem_used = mem_total.saturating_sub(mem_free);

    let swap = read_swap_usage()?;
    Ok(Stats {
        mem_total,
        mem_used,
        mem_free,
        swap_total: swap.xsu_total,
        swap_used: swap.xsu_used,
        swap_free: swap.xsu_avail,
    })
}

#[cfg(not(target_os = "macos"))]
fn read_stats() -> Result<Stats, Vec<AppletError>> {
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

#[cfg(target_os = "macos")]
fn read_sysctl_u64(name: &str) -> Result<u64, Vec<AppletError>> {
    let c_name = CString::new(name)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid key '{name}'"))])?;
    let mut value = 0_u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: `c_name` is NUL-terminated and `value` points to writable memory.
    let rc = unsafe {
        libc::sysctlbyname(
            c_name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size != std::mem::size_of::<u64>() {
        Err(vec![AppletError::new(
            APPLET,
            format!("reading sysctl '{name}' failed"),
        )])
    } else {
        Ok(value)
    }
}

#[cfg(target_os = "macos")]
fn read_swap_usage() -> Result<libc::xsw_usage, Vec<AppletError>> {
    let name = CString::new("vm.swapusage").expect("static string is NUL-free");
    let mut usage = MaybeUninit::<libc::xsw_usage>::uninit();
    let mut size = std::mem::size_of::<libc::xsw_usage>();
    // SAFETY: `usage` points to writable memory for the kernel to initialize.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            usage.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size != std::mem::size_of::<libc::xsw_usage>() {
        return Err(vec![AppletError::new(APPLET, "reading swap usage failed")]);
    }
    // SAFETY: `sysctlbyname` initialized `usage` on success.
    Ok(unsafe { usage.assume_init() })
}

#[cfg(test)]
mod tests {
    use super::{Options, Unit, parse_args, scale};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_unit_flags() {
        assert_eq!(
            parse_args(&args(&["-m"])).expect("parse free"),
            Options { unit: Unit::MiB }
        );
        assert_eq!(
            parse_args(&[]).expect("parse free"),
            Options { unit: Unit::KiB }
        );
    }

    #[test]
    fn scales_values() {
        assert_eq!(scale(4096, Unit::Bytes), 4096);
        assert_eq!(scale(4096, Unit::KiB), 4);
        assert_eq!(scale(4 * 1024 * 1024, Unit::MiB), 4);
        assert_eq!(scale(4 * 1024 * 1024 * 1024, Unit::GiB), 4);
    }
}
