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
        let mut value = self.parse_match()?;
        while let Some(op @ ("*" | "/" | "%")) = self.peek() {
            self.index += 1;
            let rhs = self.parse_match()?;
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

    fn parse_match(&mut self) -> Result<Value, Vec<AppletError>> {
        let value = self.parse_primary()?;
        if self.peek() != Some(":") {
            return Ok(value);
        }
        self.index += 1;
        let pattern = self.parse_primary()?;
        bre_apply(value.text().as_bytes(), pattern.text().as_bytes())
            .map_err(|e| vec![AppletError::new(APPLET, e)])
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

// BRE (Basic Regular Expressions) engine for the ':' operator.
// Anchored at the start of the string; returns the captured group \(...\)
// content if present, otherwise the match length.

#[derive(Debug, Clone)]
enum BreAtom {
    Lit(u8),
    Any,
    Class { negated: bool, items: Vec<BreClassItem> },
    Group(Vec<BreFrag>),
}

#[derive(Debug, Clone)]
enum BreClassItem {
    Byte(u8),
    Range(u8, u8),
}

#[derive(Debug, Clone)]
struct BreFrag {
    atom: BreAtom,
    star: bool,
}

fn bre_apply(s: &[u8], p: &[u8]) -> Result<Value, String> {
    let frags = bre_parse(p)?;
    let has_group = frags_have_group(&frags);
    let mut cap_start: Option<usize> = None;
    let mut cap_end: Option<usize> = None;

    match bre_exec(&frags, s, 0, &mut cap_start, &mut cap_end) {
        Some(match_end) => {
            if has_group {
                let text = match (cap_start, cap_end) {
                    (Some(a), Some(b)) => String::from_utf8_lossy(&s[a..b]).into_owned(),
                    _ => String::new(),
                };
                Ok(Value::Str(text))
            } else {
                Ok(Value::Int(match_end as i64))
            }
        }
        None => {
            if has_group {
                Ok(Value::Str(String::new()))
            } else {
                Ok(Value::Int(0))
            }
        }
    }
}

fn frags_have_group(frags: &[BreFrag]) -> bool {
    frags.iter().any(|f| matches!(&f.atom, BreAtom::Group(_)))
}

fn bre_parse(p: &[u8]) -> Result<Vec<BreFrag>, String> {
    let mut i = 0;
    bre_parse_frags(p, &mut i, false)
}

fn bre_parse_frags(p: &[u8], i: &mut usize, in_group: bool) -> Result<Vec<BreFrag>, String> {
    let mut frags = Vec::new();
    while *i < p.len() {
        // End of group: \)
        if in_group && p[*i] == b'\\' && *i + 1 < p.len() && p[*i + 1] == b')' {
            *i += 2;
            return Ok(frags);
        }

        let atom = if p[*i] == b'.' {
            *i += 1;
            BreAtom::Any
        } else if p[*i] == b'[' {
            *i += 1;
            let (items, negated, next) = bre_parse_class(p, *i)?;
            *i = next;
            BreAtom::Class { negated, items }
        } else if p[*i] == b'\\' && *i + 1 < p.len() {
            *i += 1;
            match p[*i] {
                b'(' => {
                    *i += 1;
                    let inner = bre_parse_frags(p, i, true)?;
                    BreAtom::Group(inner)
                }
                b => {
                    *i += 1;
                    BreAtom::Lit(b)
                }
            }
        } else {
            let b = p[*i];
            *i += 1;
            BreAtom::Lit(b)
        };

        let star = *i < p.len() && p[*i] == b'*';
        if star {
            *i += 1;
        }
        frags.push(BreFrag { atom, star });
    }

    if in_group {
        return Err("unterminated BRE group".to_owned());
    }
    Ok(frags)
}

fn bre_parse_class(p: &[u8], mut i: usize) -> Result<(Vec<BreClassItem>, bool, usize), String> {
    let negated = i < p.len() && p[i] == b'^';
    if negated {
        i += 1;
    }
    let mut items = Vec::new();
    // A `]` as the first char in the class is treated as a literal.
    let mut first = true;
    while i < p.len() {
        if !first && p[i] == b']' {
            return Ok((items, negated, i + 1));
        }
        first = false;
        if p[i] == b'\\' && i + 1 < p.len() {
            i += 1;
            items.push(BreClassItem::Byte(p[i]));
            i += 1;
        } else if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            items.push(BreClassItem::Range(p[i], p[i + 2]));
            i += 3;
        } else {
            items.push(BreClassItem::Byte(p[i]));
            i += 1;
        }
    }
    Err("unterminated character class".to_owned())
}

fn bre_exec(
    frags: &[BreFrag],
    s: &[u8],
    pos: usize,
    cs: &mut Option<usize>,
    ce: &mut Option<usize>,
) -> Option<usize> {
    if frags.is_empty() {
        return Some(pos);
    }
    let frag = &frags[0];
    let rest = &frags[1..];

    if let BreAtom::Group(inner) = &frag.atom {
        // Groups are not quantified in standard BRE; ignore frag.star.
        let save_cs = *cs;
        let save_ce = *ce;
        *cs = Some(pos);
        if let Some(end) = bre_exec(inner, s, pos, cs, ce) {
            *ce = Some(end);
            if let Some(result) = bre_exec(rest, s, end, cs, ce) {
                return Some(result);
            }
        }
        *cs = save_cs;
        *ce = save_ce;
        return None;
    }

    if frag.star {
        // Greedy: collect all positions atom* can end at, try longest first.
        let mut ends: Vec<usize> = vec![pos];
        let mut cur = pos;
        while let Some(next) = bre_match_atom(&frag.atom, s, cur) {
            if next == cur {
                break;
            }
            ends.push(next);
            cur = next;
        }
        for &end in ends.iter().rev() {
            let save_cs = *cs;
            let save_ce = *ce;
            if let Some(result) = bre_exec(rest, s, end, cs, ce) {
                return Some(result);
            }
            *cs = save_cs;
            *ce = save_ce;
        }
        return None;
    }

    let end = bre_match_atom(&frag.atom, s, pos)?;
    bre_exec(rest, s, end, cs, ce)
}

fn bre_match_atom(atom: &BreAtom, s: &[u8], pos: usize) -> Option<usize> {
    match atom {
        BreAtom::Lit(b) => {
            if pos < s.len() && s[pos] == *b {
                Some(pos + 1)
            } else {
                None
            }
        }
        BreAtom::Any => {
            if pos < s.len() {
                Some(pos + 1)
            } else {
                None
            }
        }
        BreAtom::Class { negated, items } => {
            if pos >= s.len() {
                return None;
            }
            let matched = items.iter().any(|item| match item {
                BreClassItem::Byte(b) => s[pos] == *b,
                BreClassItem::Range(lo, hi) => s[pos] >= *lo && s[pos] <= *hi,
            });
            let matched = if *negated { !matched } else { matched };
            if matched {
                Some(pos + 1)
            } else {
                None
            }
        }
        BreAtom::Group(inner) => {
            let mut dummy_cs = None;
            let mut dummy_ce = None;
            bre_exec(inner, s, pos, &mut dummy_cs, &mut dummy_ce)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Value, bre_apply, run};

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

    #[test]
    fn match_length_with_no_group() {
        // expr "Xfoo" : 'X.' → 2 (X + one char matched)
        assert_eq!(bre_apply(b"Xfoo", b"X.").unwrap(), Value::Int(2));
        // expr "Xfoo" : 'X' → 1
        assert_eq!(bre_apply(b"Xfoo", b"X").unwrap(), Value::Int(1));
        // no match → 0
        assert_eq!(bre_apply(b"Xfoo", b"Y").unwrap(), Value::Int(0));
    }

    #[test]
    fn match_group_capture() {
        // expr "Xfoo" : 'X\(.*\)' → "foo"
        assert_eq!(
            bre_apply(b"Xfoo", b"X\\(.*\\)").unwrap(),
            Value::Str("foo".to_owned())
        );
        // no match → ""
        assert_eq!(
            bre_apply(b"Xfoo", b"Y\\(.*\\)").unwrap(),
            Value::Str(String::new())
        );
    }

    #[test]
    fn match_strip_trailing_slash() {
        // autoconf pattern: strip trailing slash from "/path/"
        assert_eq!(
            bre_apply(b"X/path/", b"X\\(.*[^/]\\)").unwrap(),
            Value::Str("/path".to_owned())
        );
        // no trailing slash — still matches
        assert_eq!(
            bre_apply(b"X/path", b"X\\(.*[^/]\\)").unwrap(),
            Value::Str("/path".to_owned())
        );
        // root "/" — no non-slash char after X, no match
        assert_eq!(
            bre_apply(b"X/", b"X\\(.*[^/]\\)").unwrap(),
            Value::Str(String::new())
        );
    }

    #[test]
    fn match_double_slash() {
        // autoconf check for "//" path
        assert_eq!(
            bre_apply(b"X//", b"X\\(//\\)").unwrap(),
            Value::Str("//".to_owned())
        );
        assert_eq!(
            bre_apply(b"X/path/", b"X\\(//\\)").unwrap(),
            Value::Str(String::new())
        );
    }

    #[test]
    fn match_colon_via_parser() {
        assert_eq!(
            run(&args(&["Xfoo", ":", "X\\(.*\\)"])).unwrap(),
            Value::Str("foo".to_owned())
        );
        assert_eq!(
            run(&args(&["Xfoo", ":", "X."])).unwrap(),
            Value::Int(2)
        );
    }

    #[test]
    fn autoconf_or_chain() {
        // expr "X/path/" : 'X\(.*[^/]\)' \| "X/path/" : 'X\(//\)' \| X.
        // (shell \| becomes | when passed as separate arg)
        let result = run(&args(&[
            "X/path/", ":", "X\\(.*[^/]\\)",
            "|",
            "X/path/", ":", "X\\(//\\)",
            "|",
            "X.",
        ])).unwrap();
        assert_eq!(result, Value::Str("/path".to_owned()));
    }
}
