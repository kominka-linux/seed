use std::fmt;
use std::io;

#[derive(Debug)]
pub struct AppletError {
    applet: &'static str,
    message: String,
}

impl AppletError {
    pub fn new(applet: &'static str, message: impl Into<String>) -> Self {
        Self {
            applet,
            message: message.into(),
        }
    }

    pub fn from_io(applet: &'static str, action: &str, path: Option<&str>, err: io::Error) -> Self {
        let detail = match path {
            Some(path) => format!("{action} {path}: {err}"),
            None => format!("{action}: {err}"),
        };
        Self::new(applet, detail)
    }

    pub fn print(&self) {
        eprintln!("{}: {}", self.applet, self.message);
    }

    pub fn invalid_option(applet: &'static str, flag: char) -> Self {
        Self::new(applet, format!("invalid option -- '{flag}'"))
    }

    pub fn option_requires_arg(applet: &'static str, option: &str) -> Self {
        Self::new(applet, format!("option requires an argument -- '{option}'"))
    }

    pub fn unrecognized_option(applet: &'static str, option: &str) -> Self {
        Self::new(applet, format!("unrecognized option '{option}'"))
    }
}

impl fmt::Display for AppletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.applet, self.message)
    }
}
