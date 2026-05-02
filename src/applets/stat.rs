use std::fs;
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "stat";

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let (format, paths) = parse_args(args)?;
    let mut out = stdout();
    let mut errors = Vec::new();

    for path in &paths {
        match render_path(path, &format) {
            Ok(text) => {
                if let Err(err) = writeln!(out, "{text}") {
                    errors.push(AppletError::from_io(APPLET, "writing stdout", None, err));
                }
            }
            Err(err) => errors.push(err),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<(String, Vec<String>), Vec<AppletError>> {
    let mut format = None;
    let mut paths = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::LongOption("format", attached) => {
                let value = cursor.next_value_or_maybe_attached(attached, APPLET, "c")?;
                format = Some(value.to_owned());
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut chars = flags.chars();
                let Some(flag) = chars.next() else {
                    continue;
                };
                let attached = chars.as_str();
                match flag {
                    'c' => {
                        let value = cursor.next_value_or_attached(attached, APPLET, "c")?;
                        format = Some(value.to_owned());
                    }
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            ArgToken::Operand(arg) => paths.push(arg.to_owned()),
        }
    }

    let Some(format) = format else {
        return Err(vec![AppletError::new(
            APPLET,
            "only '-c FORMAT' is currently supported",
        )]);
    };

    if paths.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    Ok((format, paths))
}

fn render_path(path: &str, format: &str) -> Result<String, AppletError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?;
    let symlink_target = if metadata.file_type().is_symlink() {
        Some(
            fs::read_link(path)
                .map_err(|err| AppletError::from_io(APPLET, "reading", Some(path), err))?,
        )
    } else {
        None
    };

    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }

        let Some(spec) = chars.next() else {
            return Err(AppletError::new(APPLET, "unterminated format sequence"));
        };

        match spec {
            '%' => out.push('%'),
            'a' => out.push_str(&format!("{:o}", metadata.permissions().mode() & 0o7777)),
            'F' => out.push_str(file_type_name(metadata.file_type())),
            'h' => out.push_str(&metadata.nlink().to_string()),
            'n' => out.push_str(path),
            'N' => out.push_str(&quoted_name(path, symlink_target.as_deref())),
            's' => out.push_str(&metadata.size().to_string()),
            other => {
                return Err(AppletError::new(
                    APPLET,
                    format!("unsupported format sequence '%{other}'"),
                ));
            }
        }
    }

    Ok(out)
}

fn file_type_name(file_type: fs::FileType) -> &'static str {
    if file_type.is_dir() {
        "directory"
    } else if file_type.is_symlink() {
        "symbolic link"
    } else if file_type.is_file() {
        "regular file"
    } else if file_type.is_block_device() {
        "block special file"
    } else if file_type.is_char_device() {
        "character special file"
    } else if file_type.is_fifo() {
        "fifo"
    } else if file_type.is_socket() {
        "socket"
    } else {
        "unknown"
    }
}

fn quoted_name(path: &str, target: Option<&std::path::Path>) -> String {
    match target {
        Some(target) => format!("'{}' -> '{}'", path, target.display()),
        None => format!("'{path}'"),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    use super::{file_type_name, parse_args, quoted_name, render_path};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_c_format() {
        let (format, paths) = parse_args(&args(&["-c", "%s %n", "file"])).expect("parse stat");
        assert_eq!(format, "%s %n");
        assert_eq!(paths, vec!["file"]);
    }

    #[test]
    fn parses_attached_c_format() {
        let (format, paths) = parse_args(&args(&["-c%s", "file"])).expect("parse stat");
        assert_eq!(format, "%s");
        assert_eq!(paths, vec!["file"]);
    }

    #[test]
    fn quotes_symlink_name() {
        assert_eq!(
            quoted_name("link", Some(std::path::Path::new("target"))),
            "'link' -> 'target'"
        );
    }

    #[test]
    fn renders_size_mode_and_name() {
        let dir = crate::common::unix::temp_dir("stat-test");
        let path = dir.join("file");
        std::fs::write(&path, b"abc").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o640);
        std::fs::set_permissions(&path, permissions).unwrap();

        assert_eq!(
            render_path(path.to_str().unwrap(), "%s %a %n").unwrap(),
            format!("3 640 {}", path.display())
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reports_regular_file_type() {
        let dir = crate::common::unix::temp_dir("stat-type");
        let path = dir.join("file");
        std::fs::write(&path, b"x").unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(file_type_name(metadata.file_type()), "regular file");
        std::fs::remove_dir_all(dir).ok();
    }
}
