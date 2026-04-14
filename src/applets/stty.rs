use std::fs::OpenOptions;
use std::os::fd::AsRawFd;

use crate::common::applet::finish;
use crate::common::error::AppletError;

const APPLET: &str = "stty";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    machine: bool,
    device: Option<String>,
    settings: Vec<String>,
}

#[derive(Clone, Copy)]
struct FlagSetting {
    name: &'static str,
    field: FlagField,
    bit: libc::tcflag_t,
}

#[derive(Clone, Copy)]
enum FlagField {
    Input,
    Output,
    Control,
    Local,
}

#[derive(Clone, Copy)]
struct ControlCharSetting {
    name: &'static str,
    index: usize,
}

const FLAG_SETTINGS: &[FlagSetting] = &[
    FlagSetting {
        name: "echo",
        field: FlagField::Local,
        bit: libc::ECHO,
    },
    FlagSetting {
        name: "echoe",
        field: FlagField::Local,
        bit: libc::ECHOE,
    },
    FlagSetting {
        name: "echok",
        field: FlagField::Local,
        bit: libc::ECHOK,
    },
    FlagSetting {
        name: "echonl",
        field: FlagField::Local,
        bit: libc::ECHONL,
    },
    FlagSetting {
        name: "icanon",
        field: FlagField::Local,
        bit: libc::ICANON,
    },
    FlagSetting {
        name: "isig",
        field: FlagField::Local,
        bit: libc::ISIG,
    },
    FlagSetting {
        name: "iexten",
        field: FlagField::Local,
        bit: libc::IEXTEN,
    },
    FlagSetting {
        name: "opost",
        field: FlagField::Output,
        bit: libc::OPOST,
    },
    FlagSetting {
        name: "onlcr",
        field: FlagField::Output,
        bit: libc::ONLCR,
    },
    FlagSetting {
        name: "icrnl",
        field: FlagField::Input,
        bit: libc::ICRNL,
    },
    FlagSetting {
        name: "inlcr",
        field: FlagField::Input,
        bit: libc::INLCR,
    },
    FlagSetting {
        name: "igncr",
        field: FlagField::Input,
        bit: libc::IGNCR,
    },
    FlagSetting {
        name: "ixon",
        field: FlagField::Input,
        bit: libc::IXON,
    },
    FlagSetting {
        name: "ixoff",
        field: FlagField::Input,
        bit: libc::IXOFF,
    },
    FlagSetting {
        name: "parenb",
        field: FlagField::Control,
        bit: libc::PARENB,
    },
    FlagSetting {
        name: "parodd",
        field: FlagField::Control,
        bit: libc::PARODD,
    },
    FlagSetting {
        name: "clocal",
        field: FlagField::Control,
        bit: libc::CLOCAL,
    },
    FlagSetting {
        name: "cread",
        field: FlagField::Control,
        bit: libc::CREAD,
    },
    FlagSetting {
        name: "hupcl",
        field: FlagField::Control,
        bit: libc::HUPCL,
    },
];

const CONTROL_CHARS: &[ControlCharSetting] = &[
    ControlCharSetting {
        name: "intr",
        index: libc::VINTR,
    },
    ControlCharSetting {
        name: "erase",
        index: libc::VERASE,
    },
    ControlCharSetting {
        name: "kill",
        index: libc::VKILL,
    },
    ControlCharSetting {
        name: "eof",
        index: libc::VEOF,
    },
    ControlCharSetting {
        name: "start",
        index: libc::VSTART,
    },
    ControlCharSetting {
        name: "stop",
        index: libc::VSTOP,
    },
    ControlCharSetting {
        name: "susp",
        index: libc::VSUSP,
    },
];

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    let (fd, _file) = open_target(options.device.as_deref())?;
    let mut termios = read_termios(fd)?;

    if options.settings.is_empty() {
        print_state(fd, &termios, options.all || !options.machine, options.machine)?;
        return Ok(());
    }

    let mut winsize = read_winsize(fd);
    apply_settings(fd, &mut termios, winsize.as_mut(), &options.settings)?;
    write_termios(fd, &termios)?;
    if let Some(winsize) = winsize {
        write_winsize(fd, &winsize)?;
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        match arg.as_str() {
            "-a" => options.all = true,
            "-g" => options.machine = true,
            "-F" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "F")]);
                };
                index += 1;
                options.device = Some(value.clone());
            }
            _ if arg.starts_with('-') => {
                options.settings.push(arg.clone());
            }
            _ => options.settings.push(arg.clone()),
        }
    }

    if options.all && options.machine {
        return Err(vec![AppletError::new(APPLET, "options -a and -g are mutually exclusive")]);
    }

    Ok(options)
}

fn open_target(device: Option<&str>) -> Result<(libc::c_int, Option<std::fs::File>), Vec<AppletError>> {
    if let Some(device) = device {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(device)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(device), err)])?;
        Ok((file.as_raw_fd(), Some(file)))
    } else {
        Ok((libc::STDIN_FILENO, None))
    }
}

fn read_termios(fd: libc::c_int) -> Result<libc::termios, Vec<AppletError>> {
    let mut termios = zeroed_termios();
    // SAFETY: `termios` points to valid writable memory and `fd` is expected to be an open tty descriptor.
    let rc = unsafe { libc::tcgetattr(fd, &mut termios) };
    if rc == 0 {
        Ok(termios)
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("tcgetattr: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn write_termios(fd: libc::c_int, termios: &libc::termios) -> Result<(), Vec<AppletError>> {
    // SAFETY: `termios` points to a valid configuration structure for `fd`.
    let rc = unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("tcsetattr: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn read_winsize(fd: libc::c_int) -> Option<libc::winsize> {
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `winsize` points to writable memory for the ioctl.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut winsize) };
    (rc == 0).then_some(winsize)
}

fn write_winsize(fd: libc::c_int, winsize: &libc::winsize) -> Result<(), Vec<AppletError>> {
    // SAFETY: `winsize` points to a valid structure for the ioctl call.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCSWINSZ as _, winsize) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("TIOCSWINSZ: {}", std::io::Error::last_os_error()),
        )])
    }
}

fn print_state(
    fd: libc::c_int,
    termios: &libc::termios,
    human: bool,
    machine: bool,
) -> Result<(), Vec<AppletError>> {
    if machine {
        println!("{}", encode_state(fd, termios));
        return Ok(());
    }

    let winsize = read_winsize(fd);
    let speed = output_speed(termios).unwrap_or(0);
    if human {
        if let Some(winsize) = winsize {
            println!(
                "speed {} baud; rows {}; columns {};",
                speed, winsize.ws_row, winsize.ws_col
            );
        } else {
            println!("speed {} baud;", speed);
        }
        println!(
            "{} {} {} {} {}",
            render_flag(termios.c_lflag & libc::ICANON != 0, "icanon"),
            render_flag(termios.c_lflag & libc::ECHO != 0, "echo"),
            render_flag(termios.c_lflag & libc::ISIG != 0, "isig"),
            render_flag(termios.c_iflag & libc::ICRNL != 0, "icrnl"),
            render_flag(termios.c_oflag & libc::OPOST != 0, "opost")
        );
    }
    Ok(())
}

fn render_flag(enabled: bool, name: &str) -> String {
    if enabled {
        name.to_string()
    } else {
        format!("-{name}")
    }
}

fn apply_settings(
    fd: libc::c_int,
    termios: &mut libc::termios,
    mut winsize: Option<&mut libc::winsize>,
    settings: &[String],
) -> Result<(), Vec<AppletError>> {
    let mut index = 0;
    while index < settings.len() {
        let setting = &settings[index];
        index += 1;
        if setting.contains(':') && decode_state(setting, termios)? {
            continue;
        }
        if setting == "raw" {
            // SAFETY: `termios` points to a valid structure to be modified in place.
            unsafe { libc::cfmakeraw(termios) };
            continue;
        }
        if setting == "sane" {
            apply_sane(termios);
            continue;
        }
        if let Some(flag) = setting.strip_prefix('-')
            && let Some(spec) = FLAG_SETTINGS.iter().find(|spec| spec.name == flag)
        {
            set_flag(termios, spec.field, spec.bit, false);
            continue;
        }
        if let Some(spec) = FLAG_SETTINGS.iter().find(|spec| spec.name == setting) {
            set_flag(termios, spec.field, spec.bit, true);
            continue;
        }
        if let Some(bits) = char_size_bits(setting) {
            termios.c_cflag &= !libc::CSIZE;
            termios.c_cflag |= bits;
            continue;
        }
        if setting == "min" || setting == "time" || setting == "rows" || setting == "cols" {
            let Some(value) = settings.get(index) else {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("missing value for {setting}"),
                )]);
            };
            index += 1;
            apply_named_value(termios, winsize.as_deref_mut(), setting, value)?;
            continue;
        }
        if let Some(control) = CONTROL_CHARS.iter().find(|control| control.name == setting) {
            let Some(value) = settings.get(index) else {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("missing value for {setting}"),
                )]);
            };
            index += 1;
            termios.c_cc[control.index] = parse_control_char(value)?;
            continue;
        }
        if let Ok(speed) = setting.parse::<u32>() {
            set_speed(termios, speed)?;
            continue;
        }
        if setting == "ispeed" || setting == "ospeed" {
            let Some(value) = settings.get(index) else {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("missing value for {setting}"),
                )]);
            };
            index += 1;
            let speed = value.parse::<u32>().map_err(|_| {
                vec![AppletError::new(
                    APPLET,
                    format!("invalid speed '{value}'"),
                )]
            })?;
            set_named_speed(termios, setting, speed)?;
            continue;
        }
        return Err(vec![AppletError::new(
            APPLET,
            format!("unsupported setting '{setting}'"),
        )]);
    }
    let _ = fd;
    Ok(())
}

fn apply_named_value(
    termios: &mut libc::termios,
    winsize: Option<&mut libc::winsize>,
    setting: &str,
    value: &str,
) -> Result<(), Vec<AppletError>> {
    match setting {
        "min" => {
            termios.c_cc[libc::VMIN] = value.parse::<u8>().map_err(|_| {
                vec![AppletError::new(APPLET, format!("invalid value '{value}' for min"))]
            })?;
        }
        "time" => {
            termios.c_cc[libc::VTIME] = value.parse::<u8>().map_err(|_| {
                vec![AppletError::new(APPLET, format!("invalid value '{value}' for time"))]
            })?;
        }
        "rows" => {
            let winsize = winsize.ok_or_else(|| vec![AppletError::new(APPLET, "window size unavailable")])?;
            winsize.ws_row = value.parse::<u16>().map_err(|_| {
                vec![AppletError::new(APPLET, format!("invalid row count '{value}'"))]
            })?;
        }
        "cols" => {
            let winsize = winsize.ok_or_else(|| vec![AppletError::new(APPLET, "window size unavailable")])?;
            winsize.ws_col = value.parse::<u16>().map_err(|_| {
                vec![AppletError::new(APPLET, format!("invalid column count '{value}'"))]
            })?;
        }
        _ => {}
    }
    Ok(())
}

fn set_flag(termios: &mut libc::termios, field: FlagField, bit: libc::tcflag_t, enabled: bool) {
    let flags = match field {
        FlagField::Input => &mut termios.c_iflag,
        FlagField::Output => &mut termios.c_oflag,
        FlagField::Control => &mut termios.c_cflag,
        FlagField::Local => &mut termios.c_lflag,
    };
    if enabled {
        *flags |= bit;
    } else {
        *flags &= !bit;
    }
}

fn char_size_bits(name: &str) -> Option<libc::tcflag_t> {
    match name {
        "cs5" => Some(libc::CS5),
        "cs6" => Some(libc::CS6),
        "cs7" => Some(libc::CS7),
        "cs8" => Some(libc::CS8),
        _ => None,
    }
}

fn apply_sane(termios: &mut libc::termios) {
    termios.c_iflag |= libc::BRKINT | libc::ICRNL | libc::IXON;
    termios.c_iflag &= !(libc::IGNBRK | libc::IGNCR | libc::INLCR | libc::IXOFF);
    termios.c_oflag |= libc::OPOST | libc::ONLCR;
    termios.c_lflag |= libc::ISIG | libc::ICANON | libc::IEXTEN | libc::ECHO | libc::ECHOE | libc::ECHOK;
    termios.c_cflag |= libc::CREAD | libc::CS8;
    termios.c_cflag &= !(libc::PARENB | libc::PARODD);
    termios.c_cc[libc::VINTR] = 3;
    termios.c_cc[libc::VERASE] = 127;
    termios.c_cc[libc::VKILL] = 21;
    termios.c_cc[libc::VEOF] = 4;
    termios.c_cc[libc::VMIN] = 1;
    termios.c_cc[libc::VTIME] = 0;
}

fn parse_control_char(value: &str) -> Result<libc::cc_t, Vec<AppletError>> {
    if value == "undef" || value == "^-" {
        return Ok(0);
    }
    if let Some(rest) = value.strip_prefix('^') {
        if rest == "?" {
            return Ok(127);
        }
        if rest.len() == 1 {
            return Ok(rest.as_bytes()[0].to_ascii_uppercase() & 0x1f);
        }
    }
    if value.len() == 1 {
        return Ok(value.as_bytes()[0]);
    }
    Err(vec![AppletError::new(
        APPLET,
        format!("invalid control character '{value}'"),
    )])
}

fn encode_state(fd: libc::c_int, termios: &libc::termios) -> String {
    let winsize = read_winsize(fd).unwrap_or(libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    });
    let mut parts = vec![
        format!("{:x}", termios.c_iflag),
        format!("{:x}", termios.c_oflag),
        format!("{:x}", termios.c_cflag),
        format!("{:x}", termios.c_lflag),
        format!("{:x}", input_speed(termios).unwrap_or(0)),
        format!("{:x}", output_speed(termios).unwrap_or(0)),
        format!("{:x}", winsize.ws_row),
        format!("{:x}", winsize.ws_col),
    ];
    parts.extend(termios.c_cc.iter().map(|value| format!("{:x}", *value)));
    parts.join(":")
}

fn decode_state(setting: &str, termios: &mut libc::termios) -> Result<bool, Vec<AppletError>> {
    let parts = setting.split(':').collect::<Vec<_>>();
    if parts.len() < 8 {
        return Ok(false);
    }
    let parse = |value: &str| u64::from_str_radix(value, 16).ok();
    let Some(c_iflag) = parse(parts[0]) else {
        return Ok(false);
    };
    let Some(c_oflag) = parse(parts[1]) else {
        return Ok(false);
    };
    let Some(c_cflag) = parse(parts[2]) else {
        return Ok(false);
    };
    let Some(c_lflag) = parse(parts[3]) else {
        return Ok(false);
    };
    termios.c_iflag = c_iflag as libc::tcflag_t;
    termios.c_oflag = c_oflag as libc::tcflag_t;
    termios.c_cflag = c_cflag as libc::tcflag_t;
    termios.c_lflag = c_lflag as libc::tcflag_t;
    set_speed(termios, parse(parts[4]).unwrap_or(0) as u32)?;
    set_named_speed(termios, "ospeed", parse(parts[5]).unwrap_or(0) as u32)?;
    for (index, value) in parts.iter().skip(8).enumerate() {
        if index >= termios.c_cc.len() {
            break;
        }
        termios.c_cc[index] = parse(value).unwrap_or(0) as libc::cc_t;
    }
    Ok(true)
}

fn set_speed(termios: &mut libc::termios, speed: u32) -> Result<(), Vec<AppletError>> {
    set_named_speed(termios, "ispeed", speed)?;
    set_named_speed(termios, "ospeed", speed)
}

fn set_named_speed(termios: &mut libc::termios, which: &str, speed: u32) -> Result<(), Vec<AppletError>> {
    let constant = speed_constant(speed).ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            format!("unsupported speed '{speed}'"),
        )]
    })?;
    let rc = unsafe {
        match which {
            "ispeed" => libc::cfsetispeed(termios, constant),
            _ => libc::cfsetospeed(termios, constant),
        }
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("setting speed {speed} failed"),
        )])
    }
}

fn input_speed(termios: &libc::termios) -> Option<u32> {
    let speed = unsafe { libc::cfgetispeed(termios) };
    speed_value(speed)
}

fn output_speed(termios: &libc::termios) -> Option<u32> {
    let speed = unsafe { libc::cfgetospeed(termios) };
    speed_value(speed)
}

fn speed_constant(speed: u32) -> Option<libc::speed_t> {
    SPEED_TABLE
        .iter()
        .find(|(value, _)| *value == speed)
        .map(|(_, constant)| *constant)
}

fn speed_value(speed: libc::speed_t) -> Option<u32> {
    SPEED_TABLE
        .iter()
        .find(|(_, constant)| *constant == speed)
        .map(|(value, _)| *value)
}

fn zeroed_termios() -> libc::termios {
    // SAFETY: zero initialization is valid for `termios`.
    unsafe { std::mem::zeroed() }
}

const SPEED_TABLE: &[(u32, libc::speed_t)] = &[
    (0, libc::B0),
    (50, libc::B50),
    (75, libc::B75),
    (110, libc::B110),
    (134, libc::B134),
    (150, libc::B150),
    (200, libc::B200),
    (300, libc::B300),
    (600, libc::B600),
    (1200, libc::B1200),
    (1800, libc::B1800),
    (2400, libc::B2400),
    (4800, libc::B4800),
    (9600, libc::B9600),
    (19200, libc::B19200),
    (38400, libc::B38400),
    #[cfg(not(target_os = "macos"))]
    (57600, libc::B57600),
    #[cfg(not(target_os = "macos"))]
    (115200, libc::B115200),
    #[cfg(not(target_os = "macos"))]
    (230400, libc::B230400),
];

#[cfg(test)]
mod tests {
    use super::{Options, decode_state, parse_args, parse_control_char, zeroed_termios};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_device_and_settings() {
        assert_eq!(
            parse_args(&args(&["-F", "/dev/ttyS0", "-g", "raw", "-echo"])).unwrap(),
            Options {
                machine: true,
                device: Some("/dev/ttyS0".to_string()),
                settings: vec!["raw".to_string(), "-echo".to_string()],
                ..Options::default()
            }
        );
    }

    #[test]
    fn parses_control_char_caret_notation() {
        assert_eq!(parse_control_char("^C").unwrap(), 3);
        assert_eq!(parse_control_char("^?").unwrap(), 127);
    }

    #[test]
    fn decodes_machine_state() {
        let mut termios = zeroed_termios();
        assert!(decode_state("1:2:3:4:0:0:0:0:3:7f", &mut termios).unwrap());
        assert_eq!(termios.c_iflag, 1);
        assert_eq!(termios.c_oflag, 2);
        assert_eq!(termios.c_lflag, 4);
        assert_eq!(termios.c_cc[0], 3);
        assert_eq!(termios.c_cc[1], 0x7f);
    }
}
