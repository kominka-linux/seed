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
}

impl fmt::Display for AppletError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.applet, self.message)
    }
}
