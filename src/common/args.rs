use crate::common::error::AppletError;

pub enum ArgToken<'a> {
    ShortFlags(&'a str),
    Operand(&'a str),
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
    use super::{ArgCursor, ArgToken};

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
}
