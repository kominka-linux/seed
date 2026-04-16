use crate::common::error::AppletError;

pub enum ArgToken<'a> {
    ShortFlags(&'a str),
    Operand(&'a str),
}

#[derive(Debug, Eq, PartialEq)]
pub enum ParsedArg {
    Short(char),
    Long(String),
    Value(String),
}

pub struct Parser {
    applet: &'static str,
    inner: lexopt::Parser,
}

pub struct RawArgs<'a> {
    inner: lexopt::RawArgs<'a>,
}

impl Parser {
    pub fn new(applet: &'static str, args: &[String]) -> Self {
        Self {
            applet,
            inner: lexopt::Parser::from_args(args.iter().map(String::as_str)),
        }
    }

    pub fn next_arg(&mut self) -> Result<Option<ParsedArg>, Vec<AppletError>> {
        match self.inner.next() {
            Ok(Some(lexopt::Arg::Short(flag))) => Ok(Some(ParsedArg::Short(flag))),
            Ok(Some(lexopt::Arg::Long(name))) => Ok(Some(ParsedArg::Long(name.to_owned()))),
            Ok(Some(lexopt::Arg::Value(value))) => {
                Ok(Some(ParsedArg::Value(value.to_string_lossy().into_owned())))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(vec![AppletError::new(self.applet, err.to_string())]),
        }
    }

    pub fn value(&mut self, option: &str) -> Result<String, Vec<AppletError>> {
        self.inner
            .value()
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|_| vec![AppletError::option_requires_arg(self.applet, option)])
    }

    pub fn optional_value(&mut self) -> Option<String> {
        self.inner
            .optional_value()
            .map(|value| value.to_string_lossy().into_owned())
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
}

impl RawArgs<'_> {
    pub fn peek(&self) -> Option<&std::ffi::OsStr> {
        self.inner.peek()
    }

    pub fn as_slice(&self) -> &[std::ffi::OsString] {
        self.inner.as_slice()
    }

    pub fn next_if(
        &mut self,
        func: impl FnOnce(&std::ffi::OsStr) -> bool,
    ) -> Option<std::ffi::OsString> {
        self.inner.next_if(func)
    }
}

impl Iterator for RawArgs<'_> {
    type Item = std::ffi::OsString;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

pub struct ArgCursor<'a> {
    args: &'a [String],
    index: usize,
    parsing_flags: bool,
}

impl<'a> ArgCursor<'a> {
    pub fn new(args: &'a [String]) -> Self {
        Self {
            args,
            index: 0,
            parsing_flags: true,
        }
    }

    pub fn parsing_flags(&self) -> bool {
        self.parsing_flags
    }

    pub fn next_token(&mut self) -> Option<&'a str> {
        while let Some(arg) = self.args.get(self.index) {
            self.index += 1;
            if self.parsing_flags && arg == "--" {
                self.parsing_flags = false;
                continue;
            }
            return Some(arg);
        }
        None
    }

    pub fn next_arg(&mut self) -> Option<ArgToken<'a>> {
        let arg = self.next_token()?;
        if self.parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            Some(ArgToken::ShortFlags(&arg[1..]))
        } else {
            Some(ArgToken::Operand(arg))
        }
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
        Ok(arg)
    }

    pub fn next_value_or_attached(
        &mut self,
        attached: &'a str,
        applet: &'static str,
        option: &str,
    ) -> Result<&'a str, Vec<AppletError>> {
        if attached.is_empty() {
            self.next_value(applet, option)
        } else {
            Ok(attached)
        }
    }

    pub fn remaining(&self) -> &'a [String] {
        &self.args[self.index..]
    }
}

#[cfg(test)]
mod tests {
    use super::{ArgCursor, ArgToken, ParsedArg, Parser};

    #[test]
    fn skips_double_dash_for_tokens() {
        let args = vec!["-a".to_string(), "--".to_string(), "-b".to_string()];
        let mut cursor = ArgCursor::new(&args);
        assert_eq!(cursor.next_token(), Some("-a"));
        assert_eq!(cursor.next_token(), Some("-b"));
        assert!(!cursor.parsing_flags());
        assert_eq!(cursor.next_token(), None);
    }

    #[test]
    fn preserves_double_dash_as_option_value() {
        let args = vec!["-o".to_string(), "--".to_string()];
        let mut cursor = ArgCursor::new(&args);
        assert_eq!(cursor.next_token(), Some("-o"));
        assert_eq!(cursor.next_value("test", "o").expect("value"), "--");
    }

    #[test]
    fn splits_short_flags_from_operands() {
        let args = vec!["-abc".to_string(), "file".to_string()];
        let mut cursor = ArgCursor::new(&args);
        match cursor.next_arg() {
            Some(ArgToken::ShortFlags(flags)) => assert_eq!(flags, "abc"),
            _ => panic!("expected short flags"),
        }
        match cursor.next_arg() {
            Some(ArgToken::Operand(arg)) => assert_eq!(arg, "file"),
            _ => panic!("expected operand"),
        }
    }

    #[test]
    fn returns_attached_value_without_consuming_next_arg() {
        let args = vec!["-ovalue".to_string(), "file".to_string()];
        let mut cursor = ArgCursor::new(&args);
        let attached = match cursor.next_arg() {
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
            Some(ParsedArg::Value(String::from("file")))
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
        assert_eq!(parser.value("o").expect("value"), "--");
    }

    #[test]
    fn parser_reads_attached_values() {
        let args = vec!["-ovalue".to_string(), "file".to_string()];
        let mut parser = Parser::new("test", &args);
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Short('o'))
        );
        assert_eq!(parser.value("o").expect("value"), "value");
        assert_eq!(
            parser.next_arg().expect("next"),
            Some(ParsedArg::Value(String::from("file")))
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
        assert_eq!(parser.optional_value().as_deref(), Some("value"));
    }

    #[test]
    fn parser_allows_raw_arg_peeking() {
        let args = vec!["-12".to_string(), "rest".to_string()];
        let mut parser = Parser::new("test", &args);
        let raw = parser.try_raw_args().expect("raw args");
        assert_eq!(raw.peek().and_then(|arg| arg.to_str()), Some("-12"));
    }
}
