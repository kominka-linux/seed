use std::io::Write;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET_BASENAME: &str = "basename";
const APPLET_DIRNAME: &str = "dirname";

pub fn main_basename(args: &[String]) -> i32 {
    finish(run_basename(args))
}

pub fn main_dirname(args: &[String]) -> i32 {
    finish(run_dirname(args))
}

fn run_basename(args: &[String]) -> AppletResult {
    // basename [-a] [-s SUFFIX] NAME...
    // basename NAME [SUFFIX]  (traditional form)
    let mut multiple = false;
    let mut suffix: Option<&str> = None;
    let mut names: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--" => {
                i += 1;
                while i < args.len() {
                    names.push(&args[i]);
                    i += 1;
                }
                break;
            }
            "-a" => multiple = true,
            "-s" => {
                i += 1;
                if i >= args.len() {
                    return Err(vec![AppletError::option_requires_arg(APPLET_BASENAME, "s")]);
                }
                suffix = Some(&args[i]);
                multiple = true;
            }
            a if a.starts_with("-s") && a.len() > 2 => {
                suffix = Some(&args[i][2..]);
                multiple = true;
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return Err(vec![AppletError::invalid_option(
                    APPLET_BASENAME,
                    a.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => names.push(arg),
        }
        i += 1;
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
            suffix = Some(names[1]);
            names.truncate(1);
        }
    }

    let mut out = stdout();
    for name in &names {
        let base = base_name(name);
        let result = if let Some(suf) = suffix {
            base.strip_suffix(suf).unwrap_or(base)
        } else {
            base
        };
        writeln!(out, "{result}")
            .map_err(|e| vec![AppletError::from_io(APPLET_BASENAME, "writing", None, e)])?;
    }
    Ok(())
}

fn run_dirname(args: &[String]) -> AppletResult {
    let mut names: Vec<&str> = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }
        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET_DIRNAME,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }
        names.push(arg);
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
