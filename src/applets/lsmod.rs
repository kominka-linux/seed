use std::fs;
use std::io::Write;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "lsmod";
const PROC_MODULES: &str = "/proc/modules";
const PROC_TAINTED: &str = "/proc/sys/kernel/tainted";

#[derive(Debug, Eq, PartialEq)]
struct ModuleEntry {
    name: String,
    size: u64,
    used_by_count: u64,
    used_by: Option<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    if !args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "extra operand")]);
    }

    let modules = read_modules()?;
    let mut out = stdout();
    writeln!(out, "{}", header_line())
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;

    for module in modules {
        write_module(&mut out, &module)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn read_modules() -> Result<Vec<ModuleEntry>, Vec<AppletError>> {
    let proc_modules = fs::read_to_string(PROC_MODULES).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(PROC_MODULES),
            err,
        )]
    })?;
    parse_modules(&proc_modules).map_err(|err| vec![err])
}

fn parse_modules(proc_modules: &str) -> Result<Vec<ModuleEntry>, AppletError> {
    proc_modules
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_module_line)
        .collect()
}

fn parse_module_line(line: &str) -> Result<ModuleEntry, AppletError> {
    let mut fields = line.split_whitespace();
    let name = fields
        .next()
        .ok_or_else(|| AppletError::new(APPLET, format!("malformed line in {PROC_MODULES}")))?;
    let size = parse_u64_field(fields.next(), "size")?;
    let used_by_count = parse_u64_field(fields.next(), "use count")?;
    let used_by = fields
        .next()
        .ok_or_else(|| AppletError::new(APPLET, format!("malformed line in {PROC_MODULES}")))?;

    if fields.next().is_none() || fields.next().is_none() {
        return Err(AppletError::new(
            APPLET,
            format!("malformed line in {PROC_MODULES}"),
        ));
    }

    Ok(ModuleEntry {
        name: name.to_owned(),
        size,
        used_by_count,
        used_by: parse_used_by(used_by),
    })
}

fn parse_u64_field(field: Option<&str>, name: &str) -> Result<u64, AppletError> {
    let value = field
        .ok_or_else(|| AppletError::new(APPLET, format!("missing {name} in {PROC_MODULES}")))?;
    value.parse::<u64>().map_err(|_| {
        AppletError::new(
            APPLET,
            format!("invalid {name} '{value}' in {PROC_MODULES}"),
        )
    })
}

fn parse_used_by(field: &str) -> Option<String> {
    let value = field.trim_end_matches(',');
    if value == "-" {
        None
    } else {
        Some(value.to_owned())
    }
}

fn write_module(out: &mut dyn Write, module: &ModuleEntry) -> std::io::Result<()> {
    write!(
        out,
        "{:<19} {:>8}  {}",
        module.name, module.size, module.used_by_count
    )?;
    if let Some(used_by) = &module.used_by {
        write!(out, " {used_by}")?;
    }
    writeln!(out)
}

fn header_line() -> String {
    let mut header = String::from("Module                  Size  Used by");
    if taint_suffix() {
        header.push_str("    Not tainted");
    }
    header
}

fn taint_suffix() -> bool {
    fs::read_to_string(PROC_TAINTED)
        .map(|value| value.trim() == "0")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{ModuleEntry, header_line, parse_module_line, parse_modules};

    #[test]
    fn parses_module_with_dependencies() {
        assert_eq!(
            parse_module_line("bridge 311296 1 br_netfilter, Live 0x00000000").expect("module"),
            ModuleEntry {
                name: String::from("bridge"),
                size: 311296,
                used_by_count: 1,
                used_by: Some(String::from("br_netfilter")),
            }
        );
    }

    #[test]
    fn parses_module_without_dependencies() {
        assert_eq!(
            parse_module_line("lp 16384 0 - Live 0x00000000").expect("module"),
            ModuleEntry {
                name: String::from("lp"),
                size: 16384,
                used_by_count: 0,
                used_by: None,
            }
        );
    }

    #[test]
    fn parses_empty_module_list() {
        assert!(parse_modules("").expect("empty proc modules").is_empty());
    }

    #[test]
    fn header_always_starts_with_standard_columns() {
        assert!(header_line().starts_with("Module                  Size  Used by"));
    }
}
