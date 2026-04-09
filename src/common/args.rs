use crate::common::error::AppletError;

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

    pub fn remaining(&self) -> &'a [String] {
        &self.args[self.index..]
    }
}

#[cfg(test)]
mod tests {
    use super::ArgCursor;

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
}
