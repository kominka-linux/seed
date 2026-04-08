#[cfg(target_os = "linux")]
use std::ffi::CString;

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;

const APPLET: &str = "mknod";
const DEFAULT_MODE: u32 = 0o666;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeType {
    Block,
    Char,
    Fifo,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
struct Options {
    mode: u32,
    path: String,
    node_type: NodeType,
    major: Option<u32>,
    minor: Option<u32>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let options = parse_args(args)?;
    #[cfg(target_os = "linux")]
    {
        create_node(&options)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = options;
        Err(vec![AppletError::new(
            APPLET,
            "not supported on this platform",
        )])
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut mode = DEFAULT_MODE;
    let mut parsing_flags = true;
    let mut index = 0;
    let mut positional = Vec::new();

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }

        if parsing_flags && arg == "-m" {
            let Some(value) = args.get(index + 1) else {
                return Err(vec![AppletError::new(
                    APPLET,
                    "option requires an argument -- 'm'",
                )]);
            };
            mode = parse_mode(value)?;
            index += 2;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            return Err(vec![AppletError::invalid_option(
                APPLET,
                arg.chars().nth(1).unwrap_or('-'),
            )]);
        }

        positional.push(arg.clone());
        index += 1;
    }

    if positional.len() < 2 {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    }

    let node_type = parse_node_type(&positional[1])?;
    match node_type {
        NodeType::Fifo => {
            if positional.len() != 2 {
                return Err(vec![AppletError::new(
                    APPLET,
                    "fifo type does not take major and minor numbers",
                )]);
            }
            Ok(Options {
                mode,
                path: positional[0].clone(),
                node_type,
                major: None,
                minor: None,
            })
        }
        NodeType::Block | NodeType::Char => {
            if positional.len() != 4 {
                return Err(vec![AppletError::new(
                    APPLET,
                    "device type requires major and minor numbers",
                )]);
            }
            let major = parse_device_number(&positional[2], "major")?;
            let minor = parse_device_number(&positional[3], "minor")?;
            Ok(Options {
                mode,
                path: positional[0].clone(),
                node_type,
                major: Some(major),
                minor: Some(minor),
            })
        }
    }
}

fn parse_mode(text: &str) -> Result<u32, Vec<AppletError>> {
    u32::from_str_radix(text, 8)
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid mode '{text}'"))])
}

fn parse_node_type(text: &str) -> Result<NodeType, Vec<AppletError>> {
    match text {
        "b" => Ok(NodeType::Block),
        "c" | "u" => Ok(NodeType::Char),
        "p" => Ok(NodeType::Fifo),
        _ => Err(vec![AppletError::new(
            APPLET,
            format!("invalid node type '{text}'"),
        )]),
    }
}

fn parse_device_number(text: &str, label: &str) -> Result<u32, Vec<AppletError>> {
    text.parse::<u32>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid {label} number '{text}'"),
        )]
    })
}

#[cfg(target_os = "linux")]
fn create_node(options: &Options) -> AppletResult {
    let path = CString::new(options.path.as_str())
        .map_err(|_| vec![AppletError::new(APPLET, "path contains NUL byte")])?;
    let (kind_bits, device) = match options.node_type {
        NodeType::Block => (
            libc::S_IFBLK,
            libc::makedev(options.major.unwrap_or(0), options.minor.unwrap_or(0)),
        ),
        NodeType::Char => (
            libc::S_IFCHR,
            libc::makedev(options.major.unwrap_or(0), options.minor.unwrap_or(0)),
        ),
        NodeType::Fifo => (libc::S_IFIFO, 0),
    };

    let mode = libc::mode_t::try_from(kind_bits | options.mode)
        .map_err(|_| vec![AppletError::new(APPLET, "mode out of range")])?;

    // SAFETY: `path` is a valid NUL-terminated string, and `mode`/`device`
    // are plain value parameters for the kernel syscall wrapper.
    let status = unsafe { libc::mknod(path.as_ptr(), mode, device) };
    if status == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::from_io(
            APPLET,
            "creating",
            Some(options.path.as_str()),
            std::io::Error::last_os_error(),
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MODE, NodeType, parse_args};

    #[test]
    fn parses_char_device_with_mode() {
        let options = parse_args(&[
            "-m".to_string(),
            "600".to_string(),
            "null".to_string(),
            "c".to_string(),
            "1".to_string(),
            "3".to_string(),
        ])
        .expect("parse mknod args");

        assert_eq!(options.mode, 0o600);
        assert_eq!(options.path, "null");
        assert_eq!(options.node_type, NodeType::Char);
        assert_eq!(options.major, Some(1));
        assert_eq!(options.minor, Some(3));
    }

    #[test]
    fn parses_fifo_without_device_numbers() {
        let options = parse_args(&["pipe".to_string(), "p".to_string()]).expect("parse fifo");
        assert_eq!(options.mode, DEFAULT_MODE);
        assert_eq!(options.node_type, NodeType::Fifo);
        assert_eq!(options.major, None);
        assert_eq!(options.minor, None);
    }
}
