use std::io::Write;

use crate::common::applet::fail;
use crate::common::args::argv_to_strings;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "expr";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    match run(args) {
        Ok(value) => {
            let mut out = stdout();
            if writeln!(out, "{}", value.text()).is_err() {
                eprintln!("{APPLET}: writing stdout failed");
                return 2;
            }
            if out.flush().is_err() {
                eprintln!("{APPLET}: writing stdout failed");
                return 2;
            }
            if value.is_truthy() {
                0
            } else {
                1
            }
        }
        Err(errors) => fail(errors, 2),
    }
}

fn run(args: &[std::ffi::OsString]) -> Result<Value, Vec<AppletError>> {
    let args = argv_to_strings(APPLET, args)?;
    if args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let mut parser = Parser::new(&args);
    let value = parser.parse_or()?;
    if parser.peek().is_some() {
        return Err(vec![AppletError::new(APPLET, "syntax error")]);
    }
    Ok(value)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Value {
    Int(i64),
    Str(String),
}

impl Value {
    fn text(&self) -> String {
        match self {
            Self::Int(value) => value.to_string(),
            Self::Str(value) => value.clone(),
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            Self::Int(value) => *value != 0,
            Self::Str(value) => !value.is_empty() && value != "0",
        }
    }

    fn as_int(&self) -> Result<i64, Vec<AppletError>> {
        match self {
            Self::Int(value) => Ok(*value),
            Self::Str(value) => value.parse::<i64>().map_err(|_| {
                vec![AppletError::new(
                    APPLET,
                    format!("non-integer argument '{value}'"),
                )]
            }),
        }
    }
}

struct Parser<'a> {
    tokens: &'a [String],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [String]) -> Self {
        Self { tokens, index: 0 }
    }

    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.index).map(String::as_str)
    }

    fn next(&mut self) -> Option<&'a str> {
        let token = self.peek()?;
        self.index += 1;
        Some(token)
    }

    fn parse_or(&mut self) -> Result<Value, Vec<AppletError>> {
        let mut value = self.parse_and()?;
        while self.peek() == Some("|") {
            self.index += 1;
            let rhs = self.parse_and()?;
            value = if value.is_truthy() { value } else { rhs };
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<Value, Vec<AppletError>> {
        let mut value = self.parse_cmp()?;
        while self.peek() == Some("&") {
            self.index += 1;
            let rhs = self.parse_cmp()?;
            value = if value.is_truthy() && rhs.is_truthy() {
                value
            } else {
                Value::Int(0)
            };
        }
        Ok(value)
    }

    fn parse_cmp(&mut self) -> Result<Value, Vec<AppletError>> {
        let mut value = self.parse_add()?;
        while let Some(op @ ("=" | "!=" | "<" | "<=" | ">" | ">=")) = self.peek() {
            self.index += 1;
            let rhs = self.parse_add()?;
            value = Value::Int(if compare_values(&value, op, &rhs)? {
                1
            } else {
                0
            });
        }
        Ok(value)
    }

    fn parse_add(&mut self) -> Result<Value, Vec<AppletError>> {
        let mut value = self.parse_mul()?;
        while let Some(op @ ("+" | "-")) = self.peek() {
            self.index += 1;
            let rhs = self.parse_mul()?;
            let left = value.as_int()?;
            let right = rhs.as_int()?;
            value = Value::Int(match op {
                "+" => left + right,
                "-" => left - right,
                _ => unreachable!(),
            });
        }
        Ok(value)
    }

    fn parse_mul(&mut self) -> Result<Value, Vec<AppletError>> {
        let mut value = self.parse_primary()?;
        while let Some(op @ ("*" | "/" | "%")) = self.peek() {
            self.index += 1;
            let rhs = self.parse_primary()?;
            let left = value.as_int()?;
            let right = rhs.as_int()?;
            value = Value::Int(match op {
                "*" => left * right,
                "/" => {
                    if right == 0 {
                        return Err(vec![AppletError::new(APPLET, "division by zero")]);
                    }
                    left / right
                }
                "%" => {
                    if right == 0 {
                        return Err(vec![AppletError::new(APPLET, "division by zero")]);
                    }
                    left % right
                }
                _ => unreachable!(),
            });
        }
        Ok(value)
    }

    fn parse_primary(&mut self) -> Result<Value, Vec<AppletError>> {
        let Some(token) = self.next() else {
            return Err(vec![AppletError::new(APPLET, "syntax error")]);
        };

        if token == "(" {
            let value = self.parse_or()?;
            if self.next() != Some(")") {
                return Err(vec![AppletError::new(APPLET, "missing ')'")]);
            }
            return Ok(value);
        }

        if let Ok(value) = token.parse::<i64>() {
            Ok(Value::Int(value))
        } else {
            Ok(Value::Str(token.to_string()))
        }
    }
}

fn compare_values(left: &Value, op: &str, right: &Value) -> Result<bool, Vec<AppletError>> {
    Ok(match op {
        "=" => left.text() == right.text(),
        "!=" => left.text() != right.text(),
        "<" | "<=" | ">" | ">=" => match (left.as_int(), right.as_int()) {
            (Ok(left), Ok(right)) => match op {
                "<" => left < right,
                "<=" => left <= right,
                ">" => left > right,
                ">=" => left >= right,
                _ => unreachable!(),
            },
            _ => match op {
                "<" => left.text() < right.text(),
                "<=" => left.text() <= right.text(),
                ">" => left.text() > right.text(),
                ">=" => left.text() >= right.text(),
                _ => unreachable!(),
            },
        },
        _ => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("syntax error near '{op}'"),
            )]);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{run, Value};

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn computes_arithmetic() {
        assert_eq!(run(&args(&["2", "*", "3"])).unwrap(), Value::Int(6));
        assert_eq!(run(&args(&["12", "%", "5"])).unwrap(), Value::Int(2));
    }

    #[test]
    fn computes_logical_ops() {
        assert_eq!(run(&args(&["1", "|", "0"])).unwrap(), Value::Int(1));
        assert_eq!(run(&args(&["1", "&", "0"])).unwrap(), Value::Int(0));
    }

    #[test]
    fn computes_comparisons() {
        assert_eq!(
            run(&args(&["0", "<", "3000000000"])).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            run(&args(&["-9223372036854775800", "<", "9223372036854775807"])).unwrap(),
            Value::Int(1)
        );
    }
}
