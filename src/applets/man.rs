use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use crate::common::applet::finish_code;
use crate::common::error::AppletError;

const APPLET: &str = "man";
const MAN_PATH: &str = "/usr/bin/man";
const MAN_PROGRAM_ENV: &str = "SEED_MAN_PROGRAM";

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    run_with_program(&man_program(), args)
}

fn man_program() -> String {
    std::env::var(MAN_PROGRAM_ENV).unwrap_or_else(|_| MAN_PATH.to_string())
}

fn run_with_program(program: &str, args: &[String]) -> Result<i32, Vec<AppletError>> {
    let status = Command::new(program)
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
    use crate::common::test_env;

    use super::{MAN_PATH, man_program, run, run_with_program};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn missing_operand_errors() {
        assert!(run(&[]).is_err());
    }

    #[test]
    fn propagates_failure_status() {
        let code = run_with_program("/bin/sh", &args(&["-c", "exit 7"])).expect("run shell");
        assert_eq!(code, 7);
    }

    #[test]
    fn execution_failure_errors() {
        assert!(run_with_program("/definitely/missing/man", &args(&["seed"])).is_err());
    }

    #[test]
    fn default_program_is_system_man() {
        let _guard = test_env::lock();
        // SAFETY: tests serialize process-environment mutation with `test_env::lock()`.
        unsafe { std::env::remove_var(super::MAN_PROGRAM_ENV) };
        assert_eq!(man_program(), MAN_PATH);
    }

    #[test]
    fn env_override_changes_man_program() {
        let _guard = test_env::lock();
        // SAFETY: tests serialize process-environment mutation with `test_env::lock()`.
        unsafe { std::env::set_var(super::MAN_PROGRAM_ENV, "/tmp/fake-man") };
        assert_eq!(man_program(), "/tmp/fake-man");
        // SAFETY: tests serialize process-environment mutation with `test_env::lock()`.
        unsafe { std::env::remove_var(super::MAN_PROGRAM_ENV) };
    }
}
