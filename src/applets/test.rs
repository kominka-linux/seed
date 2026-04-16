use std::fs;

use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;

const APPLET_TEST: &str = "test";
const APPLET_BRACKET: &str = "[";
const APPLET_DOUBLE_BRACKET: &str = "[[";

pub fn main_test(args: &[std::ffi::OsString]) -> i32 {
    main_with_mode(APPLET_TEST, args, None)
}

pub fn main_bracket(args: &[std::ffi::OsString]) -> i32 {
    main_with_mode(APPLET_BRACKET, args, Some("]"))
}

pub fn main_double_bracket(args: &[std::ffi::OsString]) -> i32 {
    main_with_mode(APPLET_DOUBLE_BRACKET, args, Some("]]"))
}

fn main_with_mode(applet: &'static str, args: &[std::ffi::OsString], closing: Option<&str>) -> i32 {
    match evaluate(applet, args, closing) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            2
        }
    }
}

fn evaluate(
    applet: &'static str,
    args: &[std::ffi::OsString],
    closing: Option<&str>,
) -> Result<bool, Vec<AppletError>> {
    let tokens = argv_to_strings(applet, args)?;
    let tokens = strip_closing(applet, &tokens, closing)?;
    if tokens.is_empty() {
        return Ok(false);
    }

    let mut parser = Parser::new(applet, tokens);
    let result = parser.parse_or()?;
    if parser.peek().is_some() {
        return Err(vec![AppletError::new(applet, "too many arguments")]);
    }
    Ok(result)
}

fn strip_closing<'a>(
    applet: &'static str,
    args: &'a [String],
    closing: Option<&str>,
) -> Result<&'a [String], Vec<AppletError>> {
    let Some(closing) = closing else {
        return Ok(args);
    };

    match args.split_last() {
        Some((last, rest)) if last == closing => Ok(rest),
        _ => Err(vec![AppletError::new(
            applet,
            format!("missing '{closing}'"),
        )]),
    }
}

struct Parser<'a> {
    applet: &'static str,
    tokens: &'a [String],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(applet: &'static str, tokens: &'a [String]) -> Self {
        Self {
            applet,
            tokens,
            index: 0,
        }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn next(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.index += 1;
        Some(token)
    }

    fn parse_or(&mut self) -> Result<bool, Vec<AppletError>> {
        let mut value = self.parse_and()?;
        while self.peek() == Some("-o") {
            self.index += 1;
            let rhs = self.parse_and()?;
            value = value || rhs;
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<bool, Vec<AppletError>> {
        let mut value = self.parse_not()?;
        while self.peek() == Some("-a") {
            self.index += 1;
            let rhs = self.parse_not()?;
            value = value && rhs;
        }
        Ok(value)
    }

    fn parse_not(&mut self) -> Result<bool, Vec<AppletError>> {
        if self.peek() == Some("!") {
            self.index += 1;
            return Ok(!self.parse_not()?);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<bool, Vec<AppletError>> {
        let Some(first) = self.next() else {
            return Err(vec![AppletError::new(self.applet, "argument expected")]);
        };

        if let Some(second) = self.peek()
            && is_binary_operator(second)
        {
            let op = self.next().expect("peeked operator");
            let Some(third) = self.next() else {
                return Err(vec![AppletError::new(self.applet, "argument expected")]);
            };
            return eval_binary(self.applet, first, op, third);
        }

        if self.peek().is_some() && is_unary_operator(first) {
            let arg = self.next().expect("peeked operand");
            return eval_unary(first, arg);
        }

        Ok(!first.is_empty())
    }
}

fn is_unary_operator(token: &str) -> bool {
    matches!(
        token,
        "-e" | "-f" | "-d" | "-h" | "-L" | "-n" | "-z" | "-r" | "-w" | "-x" | "-s"
    )
}

fn is_binary_operator(token: &str) -> bool {
    matches!(
        token,
        "=" | "==" | "!=" | "-eq" | "-ne" | "-gt" | "-ge" | "-lt" | "-le"
    )
}

fn eval_unary(op: &str, arg: &str) -> Result<bool, Vec<AppletError>> {
    match op {
        "-e" => Ok(fs::symlink_metadata(arg).is_ok()),
        "-f" => Ok(fs::metadata(arg).is_ok_and(|meta| meta.is_file())),
        "-d" => Ok(fs::metadata(arg).is_ok_and(|meta| meta.is_dir())),
        "-h" | "-L" => {
            Ok(fs::symlink_metadata(arg).is_ok_and(|meta| meta.file_type().is_symlink()))
        }
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),
        "-s" => Ok(fs::metadata(arg).is_ok_and(|meta| meta.len() > 0)),
        "-r" => Ok(access(arg, libc::R_OK)),
        "-w" => Ok(access(arg, libc::W_OK)),
        "-x" => Ok(access(arg, libc::X_OK)),
        _ => Err(vec![AppletError::new(
            APPLET_TEST,
            format!("unsupported unary operator '{op}'"),
        )]),
    }
}

fn eval_binary(
    applet: &'static str,
    left: &str,
    op: &str,
    right: &str,
) -> Result<bool, Vec<AppletError>> {
    match op {
        "=" | "==" => Ok(left == right),
        "!=" => Ok(left != right),
        "-eq" | "-ne" | "-gt" | "-ge" | "-lt" | "-le" => {
            let left_num = parse_int(applet, left)?;
            let right_num = parse_int(applet, right)?;
            Ok(match op {
                "-eq" => left_num == right_num,
                "-ne" => left_num != right_num,
                "-gt" => left_num > right_num,
                "-ge" => left_num >= right_num,
                "-lt" => left_num < right_num,
                "-le" => left_num <= right_num,
                _ => unreachable!(),
            })
        }
        _ => Err(vec![AppletError::new(
            applet,
            format!("unsupported binary operator '{op}'"),
        )]),
    }
}

fn parse_int(applet: &'static str, text: &str) -> Result<i64, Vec<AppletError>> {
    text.parse::<i64>().map_err(|_| {
        vec![AppletError::new(
            applet,
            format!("integer expression expected: '{text}'"),
        )]
    })
}

fn access(path: &str, mode: libc::c_int) -> bool {
    let Ok(path) = std::ffi::CString::new(path) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated C string and `mode` is a plain
    // libc constant accepted by `access`.
    unsafe { libc::access(path.as_ptr(), mode) == 0 }
}

#[cfg(test)]
mod tests {
    use super::{evaluate, strip_closing};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn args_os(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn test_without_args_is_false() {
        assert!(!evaluate("test", &args_os(&[]), None).unwrap());
    }

    #[test]
    fn supports_string_and_file_predicates() {
        let dir = crate::common::unix::temp_dir("test-applet");
        let path = dir.join("file");
        std::fs::write(&path, b"x").unwrap();

        assert!(evaluate("test", &args_os(&["a", "=", "a"]), None).unwrap());
        assert!(evaluate("test", &args_os(&["-f", path.to_str().unwrap()]), None).unwrap());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn supports_boolean_operators() {
        assert!(evaluate("test", &args_os(&["a", "-a", "!", ""]), None).unwrap());
        assert!(evaluate("test", &args_os(&["", "-o", "a"]), None).unwrap());
        assert!(!evaluate("test", &args_os(&["", "-a", "a"]), None).unwrap());
    }

    #[test]
    fn bracket_requires_closing_token() {
        assert!(strip_closing("[", &args(&["a"]), Some("]")).is_err());
    }
}
