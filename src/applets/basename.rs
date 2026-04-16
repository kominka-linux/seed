use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ParsedArg, Parser};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET_BASENAME: &str = "basename";
const APPLET_DIRNAME: &str = "dirname";

pub fn main_basename(args: &[std::ffi::OsString]) -> i32 {
    finish(run_basename(args))
}

pub fn main_dirname(args: &[std::ffi::OsString]) -> i32 {
    finish(run_dirname(args))
}

fn run_basename(args: &[std::ffi::OsString]) -> AppletResult {
    // basename [-a] [-s SUFFIX] NAME...
    // basename NAME [SUFFIX]  (traditional form)
    let mut multiple = false;
    let mut suffix: Option<String> = None;
    let mut names: Vec<String> = Vec::new();
    let mut parser = Parser::new(APPLET_BASENAME, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short('a') => multiple = true,
            ParsedArg::Short('s') => {
                let value = parser
                    .optional_value_str()?
                    .unwrap_or(parser.value_str("s")?);
                suffix = Some(value);
                multiple = true;
            }
            ParsedArg::Short(flag) => {
                return Err(vec![AppletError::invalid_option(APPLET_BASENAME, flag)])
            }
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET_BASENAME,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => {
                names.push(arg.into_string().map_err(|arg| {
                    vec![AppletError::new(
                        APPLET_BASENAME,
                        format!("argument is invalid unicode: {:?}", arg),
                    )]
                })?);
            }
        }
    }

    if names.is_empty() {
        return Err(vec![AppletError::new(APPLET_BASENAME, "missing operand")]);
    }

    // Traditional two-argument form: basename NAME SUFFIX
    if !multiple {
        if names.len() > 2 {
            return Err(vec![AppletError::new(APPLET_BASENAME, "extra operand")]);
        }
        if names.len() == 2 {
            suffix = Some(names[1].clone());
            names.truncate(1);
        }
    }

    let mut out = stdout();
    for name in &names {
        let base = base_name(name);
        let result = if let Some(suf) = suffix.as_deref() {
            base.strip_suffix(suf).unwrap_or(base)
        } else {
            base
        };
        writeln!(out, "{result}")
            .map_err(|e| vec![AppletError::from_io(APPLET_BASENAME, "writing", None, e)])?;
    }
    Ok(())
}

fn run_dirname(args: &[std::ffi::OsString]) -> AppletResult {
    let mut names: Vec<String> = Vec::new();
    let mut parser = Parser::new(APPLET_DIRNAME, args);

    while let Some(arg) = parser.next_arg()? {
        match arg {
            ParsedArg::Short(flag) => {
                return Err(vec![AppletError::invalid_option(APPLET_DIRNAME, flag)])
            }
            ParsedArg::Long(name) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET_DIRNAME,
                    &format!("--{name}"),
                )])
            }
            ParsedArg::Value(arg) => {
                names.push(arg.into_string().map_err(|arg| {
                    vec![AppletError::new(
                        APPLET_DIRNAME,
                        format!("argument is invalid unicode: {:?}", arg),
                    )]
                })?);
            }
        }
    }

    if names.is_empty() {
        return Err(vec![AppletError::new(APPLET_DIRNAME, "missing operand")]);
    }

    let mut out = stdout();
    for name in &names {
        let dir = dir_name(name);
        writeln!(out, "{dir}")
            .map_err(|e| vec![AppletError::from_io(APPLET_DIRNAME, "writing", None, e)])?;
    }
    Ok(())
}

fn base_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        Some(pos) => &trimmed[pos + 1..],
        None => trimmed,
    }
}

fn dir_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        None => ".",
        Some(0) => "/",
        Some(pos) => {
            let prefix = trimmed[..pos].trim_end_matches('/');
            if prefix.is_empty() { "/" } else { prefix }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{base_name, dir_name};

    #[test]
    fn basename_cases() {
        assert_eq!(base_name("/usr/lib"), "lib");
        assert_eq!(base_name("/usr/"), "usr");
        assert_eq!(base_name("/"), "/");
        assert_eq!(base_name("foo"), "foo");
        assert_eq!(base_name(""), "/");
    }

    #[test]
    fn dirname_cases() {
        assert_eq!(dir_name("/usr/lib"), "/usr");
        assert_eq!(dir_name("/usr/"), "/");
        assert_eq!(dir_name("/"), "/");
        assert_eq!(dir_name("foo"), ".");
        assert_eq!(dir_name("foo/bar"), "foo");
        assert_eq!(dir_name("//foo//bar//"), "//foo");
    }
}
