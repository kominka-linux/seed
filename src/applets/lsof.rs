#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::Write;

use crate::common::applet::{AppletResult, finish};
#[cfg(target_os = "linux")]
use crate::common::process::{ProcessInfo, list_processes};
use crate::common::error::AppletError;
#[cfg(target_os = "linux")]
use crate::common::io::stdout;

const APPLET: &str = "lsof";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let _ = args;

    #[cfg(target_os = "linux")]
    {
        return run_linux();
    }

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

#[cfg(target_os = "linux")]
fn run_linux() -> AppletResult {
    let processes = list_processes().map_err(|message| vec![AppletError::new(APPLET, message)])?;
    let mut out = stdout();

    for process in processes {
        for descriptor in descriptors_for_process(process.pid)? {
            let target = fs::read_link(format!("/proc/{}/fd/{}", process.pid, descriptor))
                .map(|path| path.to_string_lossy().into_owned());
            let Ok(target) = target else {
                continue;
            };
            writeln!(
                out,
                "{}\t{}\t{}\t{}",
                process.pid,
                display_command(&process),
                descriptor,
                target
            )
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
        }
    }

    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn descriptors_for_process(pid: i32) -> Result<Vec<u32>, Vec<AppletError>> {
    let mut descriptors = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd")).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "reading",
            Some(&format!("/proc/{pid}/fd")),
            err,
        )]
    })? {
        let entry = entry.map_err(|err| vec![AppletError::from_io(APPLET, "reading", None, err)])?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(descriptor) = name.parse::<u32>() else {
            continue;
        };
        descriptors.push(descriptor);
    }
    descriptors.sort_unstable();
    Ok(descriptors)
}

#[cfg(target_os = "linux")]
fn display_command(process: &ProcessInfo) -> &str {
    process
        .command
        .split_whitespace()
        .next()
        .filter(|command| !command.is_empty())
        .unwrap_or(&process.name)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::display_command;
    #[cfg(target_os = "linux")]
    use crate::common::process::ProcessInfo;

    #[cfg(target_os = "linux")]
    #[test]
    fn prefers_argv0_for_command() {
        let process = ProcessInfo {
            pid: 1,
            ppid: 0,
            uid: 0,
            tty: String::from("??"),
            cpu_time_ns: 0,
            name: String::from("busybox"),
            command: String::from("/bin/busybox lsof"),
        };
        assert_eq!(display_command(&process), "/bin/busybox");
    }
}
