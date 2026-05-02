use std::ffi::{OsStr, OsString};

use crate::common::error::AppletError;

pub enum ArgToken<'a> {
    ShortFlags(&'a str),
    LongOption(&'a str, Option<&'a str>),
    Operand(&'a str),
}

pub fn split_long_option(flags: &str) -> Option<(&str, Option<&str>)> {
    let long = flags.strip_prefix('-')?;
    let Some((name, value)) = long.split_once('=') else {
        return Some((long, None));
    };
    Some((name, Some(value)))
}

pub fn split_short_option(flags: &str) -> Option<(char, &str)> {
    let mut chars = flags.char_indices();
    let (_, flag) = chars.next()?;
    let attached_start = chars.next().map_or(flags.len(), |(index, _)| index);
    Some((flag, &flags[attached_start..]))
}

#[derive(Debug, Eq, PartialEq)]
pub enum ParsedArg {
    Short(char),
    Long(String),
    Value(OsString),
}

pub struct Parser {
    applet: &'static str,
    inner: lexopt::Parser,
}

pub struct RawArgs<'a> {
    inner: lexopt::RawArgs<'a>,
}

pub fn argv_to_strings(
    applet: &'static str,
    args: &[OsString],
) -> Result<Vec<String>, Vec<AppletError>> {
    args.iter()
        .map(|arg| os_to_string(applet, arg.as_os_str()))
        .collect()
}

pub fn os_to_string(applet: &'static str, value: &OsStr) -> Result<String, Vec<AppletError>> {
    value.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        vec![AppletError::new(
            applet,
            format!("argument is invalid unicode: {:?}", value),
        )]
    })
}

impl Parser {
    pub fn new<S>(applet: &'static str, args: &[S]) -> Self
    where
        S: AsRef<OsStr>,
    {
        Self {
            applet,
            inner: lexopt::Parser::from_args(args.iter().map(|arg| arg.as_ref().to_os_string())),
        }
    }

    pub fn next_arg(&mut self) -> Result<Option<ParsedArg>, Vec<AppletError>> {
        match self.inner.next() {
            Ok(Some(lexopt::Arg::Short(flag))) => Ok(Some(ParsedArg::Short(flag))),
            Ok(Some(lexopt::Arg::Long(name))) => Ok(Some(ParsedArg::Long(name.to_owned()))),
            Ok(Some(lexopt::Arg::Value(value))) => Ok(Some(ParsedArg::Value(value))),
            Ok(None) => Ok(None),
            Err(err) => Err(vec![AppletError::new(self.applet, err.to_string())]),
        }
    }

    pub fn value_os(&mut self, option: &str) -> Result<OsString, Vec<AppletError>> {
        self.inner
            .value()
            .map_err(|_| vec![AppletError::option_requires_arg(self.applet, option)])
    }

    pub fn value_str(&mut self, option: &str) -> Result<String, Vec<AppletError>> {
        let value = self.value_os(option)?;
        self.os_value_to_string(value)
    }

    pub fn optional_value_os(&mut self) -> Option<OsString> {
        self.inner.optional_value()
    }

    pub fn optional_value_str(&mut self) -> Result<Option<String>, Vec<AppletError>> {
        self.optional_value_os()
            .map(|value| self.os_value_to_string(value))
            .transpose()
    }

    pub fn raw_args(&mut self) -> Result<RawArgs<'_>, Vec<AppletError>> {
        self.inner
            .raw_args()
            .map(|inner| RawArgs { inner })
            .map_err(|err| vec![AppletError::new(self.applet, err.to_string())])
    }

    pub fn try_raw_args(&mut self) -> Option<RawArgs<'_>> {
        self.inner.try_raw_args().map(|inner| RawArgs { inner })
    }

    pub fn set_short_equals(&mut self, on: bool) {
        self.inner.set_short_equals(on);
    }

    fn os_value_to_string(&self, value: OsString) -> Result<String, Vec<AppletError>> {
        value.into_string().map_err(|value| {
            vec![AppletError::new(
                self.applet,
                format!("argument is invalid unicode: {:?}", value),
            )]
        })
    }
}

impl RawArgs<'_> {
    pub fn peek(&self) -> Option<&OsStr> {
        self.inner.peek()
    }

    pub fn as_slice(&self) -> &[OsString] {
        self.inner.as_slice()
    }

    pub fn next_if(&mut self, func: impl FnOnce(&OsStr) -> bool) -> Option<OsString> {
        self.inner.next_if(func)
    }
}

impl Iterator for RawArgs<'_> {
    type Item = OsString;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct ArgCursor<'a, S: AsRef<OsStr>> {
    args: &'a [S],
    index: usize,
    parsing_flags: bool,
}

impl<'a, S: AsRef<OsStr>> ArgCursor<'a, S> {
    pub fn new(args: &'a [S]) -> Self {
        Self {
            args,
            index: 0,
            parsing_flags: true,
        }
    }

    pub fn parsing_flags(&self) -> bool {
        self.parsing_flags
    }

    pub fn next_os(&mut self) -> Option<&'a OsStr> {
        while let Some(arg) = self.args.get(self.index) {
            self.index += 1;
            if self.parsing_flags && arg.as_ref() == OsStr::new("--") {
                self.parsing_flags = false;
                continue;
            }
            return Some(arg.as_ref());
        }
        None
    }

    pub fn next_token(
        &mut self,
        applet: &'static str,
    ) -> Result<Option<&'a str>, Vec<AppletError>> {
        self.next_os()
            .map(|arg| {
                arg.to_str().ok_or_else(|| {
                    vec![AppletError::new(
                        applet,
                        format!("argument is invalid unicode: {:?}", arg),
                    )]
                })
            })
            .transpose()
    }

    pub fn next_arg(
        &mut self,
        applet: &'static str,
    ) -> Result<Option<ArgToken<'a>>, Vec<AppletError>> {
        let Some(arg) = self.next_token(applet)? else {
            return Ok(None);
        };
        Ok(Some(
            if self.parsing_flags && arg.starts_with("--") && arg.len() > 2 {
                let long = &arg[2..];
                let (name, attached) = match long.split_once('=') {
                    Some((name, value)) => (name, Some(value)),
                    None => (long, None),
                };
                ArgToken::LongOption(name, attached)
            } else if self.parsing_flags && arg.starts_with('-') && arg.len() > 1 {
                ArgToken::ShortFlags(&arg[1..])
            } else {
                ArgToken::Operand(arg)
            },
        ))
    }

    pub fn next_value(
        &mut self,
        applet: &'static str,
        option: &str,
    ) -> Result<&'a str, Vec<AppletError>> {
        let Some(arg) = self.args.get(self.index) else {
            return Err(vec![AppletError::option_requires_arg(applet, option)]);
        };
        self.index += 1;
        arg.as_ref().to_str().ok_or_else(|| {
            vec![AppletError::new(
                applet,
                format!("argument is invalid unicode: {:?}", arg.as_ref()),
            )]
        })
    }

    pub fn next_value_or_attached(
        &mut self,
        attached: &'a str,
        applet: &'static str,
        option: &str,
    ) -> Result<&'a str, Vec<AppletError>> {
        self.next_value_or_maybe_attached(Some(attached), applet, option)
    }

    pub fn next_value_or_maybe_attached(
        &mut self,
        attached: Option<&'a str>,
        applet: &'static str,
        option: &str,
    ) -> Result<&'a str, Vec<AppletError>> {
        match attached {
            Some(value) if !value.is_empty() => Ok(value),
            _ => self.next_value(applet, option),
        }
    }

    pub fn next_value_os(
        &mut self,
        applet: &'static str,
        option: &str,
    ) -> Result<OsString, Vec<AppletError>> {
        let Some(arg) = self.args.get(self.index) else {
            return Err(vec![AppletError::option_requires_arg(applet, option)]);
        };
        self.index += 1;
        Ok(arg.as_ref().to_os_string())
    }

    pub fn remaining(&self) -> &'a [S] {
        &self.args[self.index..]
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgCursor, ArgToken, ParsedArg, Parser, split_long_option, split_short_option};
    use std::ffi::OsString;

    #[test]
    fn skips_double_dash_for_tokens() {
        let args = vec!["-a".to_string(), "--".to_string(), "-b".to_string()];
        let mut cursor = ArgCursor::new(&args);
        assert_eq!(cursor.next_token("test").expect("token"), Some("-a"));
        assert_eq!(cursor.next_token("test").expect("token"), Some("-b"));
        assert!(!cursor.parsing_flags());
        assert_eq!(cursor.next_token("test").expect("token"), None);
    }

    #[test]
    fn preserves_double_dash_as_option_value() {
        let args = vec!["-o".to_string(), "--".to_string()];
        let mut cursor = ArgCursor::new(&args);
        assert_eq!(cursor.next_token("test").expect("token"), Some("-o"));
        assert_eq!(cursor.next_value("test", "o").expect("value"), "--");
    }

    #[test]
    fn splits_short_flags_from_operands() {
        let args = vec!["-abc".to_string(), "file".to_string()];
        let mut cursor = ArgCursor::new(&args);
        match cursor.next_arg("test").expect("arg") {
            Some(ArgToken::ShortFlags(flags)) => assert_eq!(flags, "abc"),
            _ => panic!("expected short flags"),
        }
        match cursor.next_arg("test").expect("arg") {
            Some(ArgToken::Operand(arg)) => assert_eq!(arg, "file"),
            _ => panic!("expected operand"),
        }
    }

    #[test]
    fn returns_attached_value_without_consuming_next_arg() {
        let args = vec!["-ovalue".to_string(), "file".to_string()];
        let mut cursor = ArgCursor::new(&args);
        let attached = match cursor.next_arg("test").expect("arg") {
            Some(ArgToken::ShortFlags(flags)) => &flags[1..],
            _ => panic!("expected short flags"),
        };
        assert_eq!(
            cursor
                .next_value_or_attached(attached, "test", "o")
                .expect("value"),
            "value"
        );
        assert_eq!(cursor.remaining(), &[String::from("file")]);
    }

    #[test]
    fn parses_long_option_with_optional_attached_value() {
        let args = vec!["--flag=value".to_string(), "--other".to_string()];
        let mut cursor = ArgCursor::new(&args);
        match cursor.next_arg("test").expect("arg") {
            Some(ArgToken::LongOption(name, Some(value))) => {
                assert_eq!(name, "flag");
                assert_eq!(value, "value");
            }
            _ => panic!("expected long option"),
        }
        match cursor.next_arg("test").expect("arg") {
            Some(ArgToken::LongOption(name, None)) => assert_eq!(name, "other"),
            _ => panic!("expected long option"),
        }
    }

    #[test]
    fn parser_splits_short_clusters() {
        let args = vec!["-abc".to_string(), "file".to_string()];
        let mut parser = Parser::new("test", &args);
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('a'))
        );
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('b'))
        );
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('c'))
        );
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Value(OsString::from("file")))
        );
        assert_eq!(parser.next_arg().expect("next"), None);
    }

    #[test]
    fn parser_preserves_double_dash_as_option_value() {
        let args = vec!["-o".to_string(), "--".to_string()];
        let mut parser = Parser::new("test", &args);
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('o'))
        );
        assert_eq!(parser.value_str("o").expect("value"), "--");
    }

    #[test]
    fn parser_reads_attached_values() {
        let args = vec!["-ovalue".to_string(), "file".to_string()];
        let mut parser = Parser::new("test", &args);
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('o'))
        );
        assert_eq!(parser.value_str("o").expect("value"), "value");
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Value(OsString::from("file")))
        );
    }

    #[test]
    fn parser_exposes_long_option_values() {
        let args = vec!["--flag=value".to_string()];
        let mut parser = Parser::new("test", &args);
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Long(String::from("flag")))
        );
        assert_eq!(
            parser.optional_value_str().expect("optional").as_deref(),
            Some("value")
        );
    }

    #[test]
    fn parser_allows_raw_arg_peeking() {
        let args = vec!["-12".to_string(), "rest".to_string()];
        let mut parser = Parser::new("test", &args);
        let raw = parser.try_raw_args().expect("raw args");
        assert_eq!(raw.peek().and_then(|arg| arg.to_str()), Some("-12"));
    }

    #[test]
    fn splits_long_option_with_attached_value() {
        assert_eq!(split_long_option("-fields=1"), Some(("fields", Some("1"))));
        assert_eq!(split_long_option("-fields"), Some(("fields", None)));
    }

    #[test]
    fn splits_short_option_with_attached_value() {
        assert_eq!(split_short_option("f1"), Some(('f', "1")));
        assert_eq!(split_short_option("s"), Some(('s', "")));
    }
}
