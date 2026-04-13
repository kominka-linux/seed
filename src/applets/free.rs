use std::fs;
use std::io::Write;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "free";
const PROC_MEMINFO: &str = "/proc/meminfo";

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Stats {
    mem_total: u64,
    mem_used: u64,
    mem_free: u64,
    swap_total: u64,
    swap_used: u64,
    swap_free: u64,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
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

fn read_stats() -> Result<Stats, Vec<AppletError>> {
    let meminfo = fs::read_to_string(PROC_MEMINFO).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(PROC_MEMINFO),
            err,
        )]
    })?;
    parse_meminfo(&meminfo).map_err(|err| vec![err])
}

fn parse_meminfo(meminfo: &str) -> Result<Stats, AppletError> {
    let mut mem_total_kib = None;
    let mut mem_free_kib = None;
    let mut swap_total_kib = None;
    let mut swap_free_kib = None;

    for line in meminfo.lines() {
        let Some((key, value_kib)) = parse_meminfo_line(line)? else {
            continue;
        };
        match key {
            "MemTotal" => mem_total_kib = Some(value_kib),
            "MemFree" => mem_free_kib = Some(value_kib),
            "SwapTotal" => swap_total_kib = Some(value_kib),
            "SwapFree" => swap_free_kib = Some(value_kib),
            _ => {}
        }
    }

    let mem_total = required_meminfo_value("MemTotal", mem_total_kib)?;
    let mem_free = required_meminfo_value("MemFree", mem_free_kib)?;
    let swap_total = required_meminfo_value("SwapTotal", swap_total_kib)?;
    let swap_free = required_meminfo_value("SwapFree", swap_free_kib)?;

    Ok(Stats {
        mem_total: mem_total * 1024,
        mem_used: mem_total.saturating_sub(mem_free) * 1024,
        mem_free: mem_free * 1024,
        swap_total: swap_total * 1024,
        swap_used: swap_total.saturating_sub(swap_free) * 1024,
        swap_free: swap_free * 1024,
    })
}

fn parse_meminfo_line(line: &str) -> Result<Option<(&str, u64)>, AppletError> {
    let Some((key, rest)) = line.split_once(':') else {
        return Ok(None);
    };
    let mut parts = rest.split_whitespace();
    let Some(value) = parts.next() else {
        return Ok(None);
    };
    let Some(unit) = parts.next() else {
        return Ok(None);
    };
    if unit != "kB" {
        return Ok(None);
    }
    let value = value.parse::<u64>().map_err(|_| {
        AppletError::new(
            APPLET,
            format!("invalid numeric value for '{key}' in {PROC_MEMINFO}"),
        )
    })?;
    Ok(Some((key, value)))
}

fn required_meminfo_value(name: &str, value: Option<u64>) -> Result<u64, AppletError> {
    value.ok_or_else(|| AppletError::new(APPLET, format!("missing '{name}' in {PROC_MEMINFO}")))
}

#[cfg(test)]
mod tests {
    use super::{Options, Stats, Unit, parse_args, parse_meminfo, scale};

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
            parse_args(&args(&[])).expect("parse free"),
            Options { unit: Unit::KiB }
        );
    }

    #[test]
    fn parses_linux_meminfo() {
        let stats = parse_meminfo(
            "\
MemTotal:        4096 kB
MemFree:         1024 kB
SwapTotal:       2048 kB
SwapFree:         512 kB
",
        )
        .expect("parse meminfo");

        assert_eq!(
            stats,
            Stats {
                mem_total: 4096 * 1024,
                mem_used: 3072 * 1024,
                mem_free: 1024 * 1024,
                swap_total: 2048 * 1024,
                swap_used: 1536 * 1024,
                swap_free: 512 * 1024,
            }
        );
    }

    #[test]
    fn rejects_missing_meminfo_fields() {
        let error = parse_meminfo("MemTotal: 4096 kB\nMemFree: 1024 kB\n")
            .expect_err("missing swap fields should fail");
        assert_eq!(
            error.to_string(),
            "free: missing 'SwapTotal' in /proc/meminfo"
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
