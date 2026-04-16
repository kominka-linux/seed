use std::fmt;
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppletErrorKind {
    Other,
    Io,
    Syscall,
    Config,
    InvalidInput,
    InvalidData,
}

#[derive(Debug)]
pub struct AppletError {
    applet: &'static str,
    message: String,
    kind: AppletErrorKind,
}

impl AppletError {
    pub fn invalid_option_message(flag: char) -> String {
        format!("invalid option -- '{flag}'")
    }

    pub fn option_requires_arg_message(option: &str) -> String {
        format!("option requires an argument -- '{option}'")
    }

    pub fn unrecognized_option_message(option: &str) -> String {
        format!("unrecognized option '{option}'")
    }

    pub fn new(applet: &'static str, message: impl Into<String>) -> Self {
        Self::new_with_kind(applet, message, AppletErrorKind::Other)
    }

    pub fn new_with_kind(
        applet: &'static str,
        message: impl Into<String>,
        kind: AppletErrorKind,
    ) -> Self {
        Self {
            applet,
            message: message.into(),
            kind,
        }
    }

    pub fn from_io(applet: &'static str, action: &str, path: Option<&str>, err: io::Error) -> Self {
        let detail = match path {
            Some(path) => format!("{action} {path}: {err}"),
            None => format!("{action}: {err}"),
        };
        Self::new_with_kind(applet, detail, AppletErrorKind::Io)
    }

    pub fn from_syscall(applet: &'static str, syscall: &str, err: io::Error) -> Self {
        Self::new_with_kind(
            applet,
            format!("{syscall} failed: {err}"),
            AppletErrorKind::Syscall,
        )
    }

    pub fn from_config(applet: &'static str, context: &str, message: impl Into<String>) -> Self {
        Self::new_with_kind(
            applet,
            format!("{context}: {}", message.into()),
            AppletErrorKind::Config,
        )
    }

    pub fn invalid_data(applet: &'static str, message: impl Into<String>) -> Self {
        Self::new_with_kind(applet, message, AppletErrorKind::InvalidData)
    }

    pub fn kind(&self) -> AppletErrorKind {
        self.kind
    }

    pub fn remap_applet(self, applet: &'static str) -> Self {
        Self::new_with_kind(applet, self.to_string(), self.kind)
    }

    pub fn print(&self) {
        eprintln!("{}: {}", self.applet, self.message);
    }

    pub fn invalid_option(applet: &'static str, flag: char) -> Self {
        Self::new_with_kind(
            applet,
            Self::invalid_option_message(flag),
            AppletErrorKind::InvalidInput,
        )
    }

    pub fn option_requires_arg(applet: &'static str, option: &str) -> Self {
        Self::new_with_kind(
            applet,
            Self::option_requires_arg_message(option),
            AppletErrorKind::InvalidInput,
        )
    }

    pub fn unrecognized_option(applet: &'static str, option: &str) -> Self {
        Self::new_with_kind(
            applet,
            Self::unrecognized_option_message(option),
            AppletErrorKind::InvalidInput,
        )
    }
}

impl fmt::Display for AppletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.applet, self.message)
    }
}
