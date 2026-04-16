use std::env;
use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "pwd";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let mut logical = true;
    let mut parser = Parser::new(APPLET, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('L') => logical = true,
            ParsedArg::Short('P') => logical = false,
            ParsedArg::Short(flag) => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(_) => {}
        }
    }

    let current_dir = env::current_dir().map_err(|e| {
        vec![AppletError::from_io(
            APPLET,
            "getting current directory",
            None,
            e,
        )]
    })?;
    let env_pwd_var = env::var_os("PWD");
    let env_pwd = env_pwd_var.as_deref().map(std::path::Path::new);
    let path = resolve_pwd(logical, env_pwd, &current_dir);

    let mut out = stdout();
    writeln!(out, "{path}").map_err(|e| vec![AppletError::from_io(APPLET, "writing", None, e)])?;
    Ok(())
}

fn resolve_pwd(
    logical: bool,
    env_pwd: Option<&std::path::Path>,
    current_dir: &std::path::Path,
) -> String {
    if logical {
        env_pwd
            .filter(|p| p.is_absolute())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| current_dir.display().to_string())
    } else {
        current_dir.display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::resolve_pwd;

    #[test]
    fn logical_mode_prefers_absolute_pwd() {
        let current = Path::new("/real/path");
        let env_pwd = Path::new("/logical/path");
        assert_eq!(resolve_pwd(true, Some(env_pwd), current), "/logical/path");
    }

    #[test]
    fn logical_mode_ignores_relative_pwd() {
        let current = Path::new("/real/path");
        let env_pwd = Path::new("relative/path");
        assert_eq!(resolve_pwd(true, Some(env_pwd), current), "/real/path");
    }

    #[test]
    fn physical_mode_uses_current_dir() {
        let current = Path::new("/real/path");
        let env_pwd = Path::new("/logical/path");
        assert_eq!(resolve_pwd(false, Some(env_pwd), current), "/real/path");
    }
}
