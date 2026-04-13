use std::io::Write;

use crate::common::error::AppletError;
use crate::common::io::stdout;
use crate::common::process::list_processes;

const APPLET: &str = "ps";

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
    parse_args(args)?;
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let mut out = stdout();
    writeln!(out, "{:>5} {:<12} {:>9} CMD", "PID", "TTY", "TIME")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    for process in processes {
        writeln!(
            out,
            "{:>5} {:<12} {:>9} {}",
            process.pid,
            process.tty,
            format_cpu_time(process.cpu_time_ns),
            process.command
        )
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    }
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<(), Vec<AppletError>> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(vec![AppletError::new(APPLET, "extra operand")])
    }
}

fn format_cpu_time(nanos: u64) -> String {
    let total_hundredths = nanos / 10_000_000;
    let total_seconds = total_hundredths / 100;
    let hundredths = total_hundredths % 100;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}.{hundredths:02}")
}

#[cfg(test)]
mod tests {
    use super::{format_cpu_time, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn rejects_extra_operands() {
        assert!(parse_args(&args(&["extra"])).is_err());
    }

    #[test]
    fn formats_cpu_time() {
        assert_eq!(format_cpu_time(0), "0:00.00");
        assert_eq!(format_cpu_time(17_366_100_000), "0:17.36");
        assert_eq!(format_cpu_time(1_046_710_000_000), "17:26.71");
    }
}
