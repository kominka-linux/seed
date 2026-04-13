use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::error::AppletError;

const APPLET: &str = "man";

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let status = Command::new("/usr/bin/man")
        .args(args)
        .status()
        .map_err(|err| {
            vec![AppletError::new(
                APPLET,
                format!("failed to execute 'man': {err}"),
            )]
        })?;

    Ok(exit_code(status))
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        code
    } else {
        128 + status.signal().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn missing_operand_errors() {
        assert!(run(&[]).is_err());
    }

    #[test]
    fn propagates_failure_status() {
        let code = run(&args(&["definitely-not-a-real-man-page"])).expect("run man");
        assert_ne!(code, 0);
    }
}
