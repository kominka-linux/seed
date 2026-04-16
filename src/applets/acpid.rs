use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::applet::finish;
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;

const APPLET: &str = "acpid";
const DEFAULT_CONF_DIR: &str = "/etc/acpi";
const DEFAULT_PROC_EVENT: &str = "/proc/acpi/event";
const DEFAULT_LOG_FILE: &str = "/var/log/acpid.log";
const DEFAULT_ACTION_FILE: &str = "/etc/acpid.conf";
const DEFAULT_MAP_FILE: &str = "/etc/acpi.map";
const DEFAULT_PID_FILE: &str = "/var/run/acpid.pid";

static RUNNING: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    foreground: bool,
    debug: bool,
    conf_dir: PathBuf,
    event_file: Option<PathBuf>,
    log_file: PathBuf,
    action_file: PathBuf,
    map_file: PathBuf,
    pid_file: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActionRule {
    key: String,
    action: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MapRule {
    event_type: u16,
    event_code: u16,
    event_value: i32,
    desc: String,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct InputEvent {
    time: libc::timeval,
    type_: u16,
    code: u16,
    value: i32,
}

enum InputSource {
    Text(BufReader<File>),
    Binary { file: File },
}

impl InputSource {
    fn fd(&self) -> libc::c_int {
        match self {
            Self::Text(reader) => reader.get_ref().as_raw_fd(),
            Self::Binary { file } => file.as_raw_fd(),
        }
    }
}

enum Logger {
    Stderr,
    File(File),
}

impl Logger {
    fn stderr() -> Self {
        Self::Stderr
    }

    fn file(path: &Path) -> Result<Self, Vec<AppletError>> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&path.to_string_lossy()), err)])?;
        Ok(Self::File(file))
    }

    fn log(&mut self, message: &str) -> Result<(), Vec<AppletError>> {
        match self {
            Self::Stderr => {
                eprintln!("{message}");
                Ok(())
            }
            Self::File(file) => {
                writeln!(file, "{message}")
                    .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
                file.flush()
                    .map_err(|err| vec![AppletError::from_io(APPLET, "flushing", None, err)])
            }
        }
    }
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    if !options.foreground {
        daemonize()?;
    }
    let mut logger = if options.debug {
        Logger::stderr()
    } else {
        Logger::file(&options.log_file)?
    };
    let _sig_guard = SignalGuard::install();
    let _pid_guard = PidFileGuard::create(&options.pid_file)?;
    let action_rules = read_action_rules(&options.action_file)?;
    let map_rules = read_map_rules(&options.map_file)?;
    let mut inputs = open_inputs(&options)?;
    if inputs.is_empty() {
        return Err(vec![AppletError::new(APPLET, "no ACPI event source found")]);
    }

    while RUNNING.load(Ordering::Relaxed) {
        if inputs.is_empty() {
            break;
        }
        let ready = poll_inputs(&inputs)?;
        if ready.is_empty() {
            continue;
        }

        let mut to_remove = Vec::new();
        for index in ready.into_iter().rev() {
            let outcome = match &mut inputs[index] {
                InputSource::Text(reader) => handle_text_input(
                    reader,
                    &options.conf_dir,
                    &action_rules,
                    &map_rules,
                    &mut logger,
                ),
                InputSource::Binary { file } => handle_binary_input(
                    file,
                    &options.conf_dir,
                    &action_rules,
                    &map_rules,
                    &mut logger,
                ),
            };

            match outcome {
                Ok(InputStatus::Continue) => {}
                Ok(InputStatus::EndOfFile) => to_remove.push(index),
                Err(errors) => {
                    for error in errors {
                        let _ = logger.log(&error.to_string());
                    }
                    to_remove.push(index);
                }
            }
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for index in to_remove.into_iter().rev() {
            inputs.remove(index);
        }
    }

    Ok(())
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options {
        foreground: false,
        debug: false,
        conf_dir: PathBuf::from(DEFAULT_CONF_DIR),
        event_file: None,
        log_file: PathBuf::from(DEFAULT_LOG_FILE),
        action_file: PathBuf::from(DEFAULT_ACTION_FILE),
        map_file: PathBuf::from(DEFAULT_MAP_FILE),
        pid_file: PathBuf::from(DEFAULT_PID_FILE),
    };
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-d" => {
                options.debug = true;
                options.foreground = true;
            }
            "-f" => options.foreground = true,
            "-c" => options.conf_dir = PathBuf::from(cursor.next_value(APPLET, "c")?),
            "-e" => options.event_file = Some(PathBuf::from(cursor.next_value(APPLET, "e")?)),
            "-l" => options.log_file = PathBuf::from(cursor.next_value(APPLET, "l")?),
            "-a" => options.action_file = PathBuf::from(cursor.next_value(APPLET, "a")?),
            "-M" => options.map_file = PathBuf::from(cursor.next_value(APPLET, "M")?),
            "-p" => options.pid_file = PathBuf::from(cursor.next_value(APPLET, "p")?),
            "-g" | "-m" | "-s" | "-S" | "-v" => {
                if matches!(arg, "-g" | "-m" | "-s" | "-S") && !cursor.remaining().is_empty() {
                    let _ = cursor.next_value(APPLET, &arg[1..])?;
                }
            }
            _ if cursor.parsing_flags() && arg.starts_with('-') => {
                return Err(vec![AppletError::unrecognized_option(APPLET, arg)]);
            }
            _ => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }

    Ok(options)
}

fn daemonize() -> Result<(), Vec<AppletError>> {
    let rc = unsafe { libc::daemon(0, 1) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::from_io(
            APPLET,
            "daemonizing",
            None,
            io::Error::last_os_error(),
        )])
    }
}

fn read_action_rules(path: &Path) -> Result<Vec<ActionRule>, Vec<AppletError>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(default_action_rules());
        }
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.to_string_lossy()),
                err,
            )]);
        }
    };

    let mut rules = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let action = parts.collect::<Vec<_>>().join(" ");
        if action.is_empty() {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid action line {}: '{line}'", line_number + 1),
            )]);
        }
        rules.push(ActionRule {
            key: key.to_string(),
            action,
        });
    }
    Ok(rules)
}

fn default_action_rules() -> Vec<ActionRule> {
    vec![
        ActionRule {
            key: String::from("PWRF"),
            action: String::from("PWRF/00000080"),
        },
        ActionRule {
            key: String::from("PWRB"),
            action: String::from("PWRB/00000080"),
        },
        ActionRule {
            key: String::from("LID0"),
            action: String::from("LID/00000080"),
        },
    ]
}

fn read_map_rules(path: &Path) -> Result<Vec<MapRule>, Vec<AppletError>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Ok(default_map_rules());
        }
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.to_string_lossy()),
                err,
            )]);
        }
    };

    let mut rules = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid map line {}: '{line}'", line_number + 1),
            )]);
        }
        let event_type = u16::from_str_radix(fields[1], 16).or_else(|_| fields[1].parse()).map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid map line {}: '{line}'", line_number + 1),
            )]
        })?;
        let event_code = fields[3].parse::<u16>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid map line {}: '{line}'", line_number + 1),
            )]
        })?;
        let event_value = fields[4].parse::<i32>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid map line {}: '{line}'", line_number + 1),
            )]
        })?;
        rules.push(MapRule {
            event_type,
            event_code,
            event_value,
            desc: fields[5..].join(" "),
        });
    }
    Ok(rules)
}

fn default_map_rules() -> Vec<MapRule> {
    vec![
        MapRule {
            event_type: 0x1,
            event_code: 116,
            event_value: 1,
            desc: String::from("button/power PWRF 00000080"),
        },
        MapRule {
            event_type: 0x1,
            event_code: 116,
            event_value: 0,
            desc: String::from("button/power PWRB 00000080"),
        },
        MapRule {
            event_type: 0x5,
            event_code: 0,
            event_value: 1,
            desc: String::from("button/lid LID0 00000080"),
        },
    ]
}

fn open_inputs(options: &Options) -> Result<Vec<InputSource>, Vec<AppletError>> {
    if let Some(path) = &options.event_file {
        let file = File::open(path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&path.to_string_lossy()), err)])?;
        return Ok(vec![InputSource::Text(BufReader::new(file))]);
    }

    let proc_event = Path::new(DEFAULT_PROC_EVENT);
    if proc_event.exists() {
        let file = File::open(proc_event)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(DEFAULT_PROC_EVENT), err)])?;
        return Ok(vec![InputSource::Text(BufReader::new(file))]);
    }

    let mut inputs = Vec::new();
    let event_dir = Path::new("/dev/input");
    let Ok(entries) = fs::read_dir(event_dir) else {
        return Ok(inputs);
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with("event"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let file = File::open(&path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&path.to_string_lossy()), err)])?;
        inputs.push(InputSource::Binary { file });
    }
    Ok(inputs)
}

fn poll_inputs(inputs: &[InputSource]) -> Result<Vec<usize>, Vec<AppletError>> {
    let mut pollfds = inputs
        .iter()
        .map(|input| libc::pollfd {
            fd: input.fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect::<Vec<_>>();
    let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1) };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted && RUNNING.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }
        return Err(vec![AppletError::from_io(APPLET, "polling", None, err)]);
    }
    Ok(pollfds
        .iter()
        .enumerate()
        .filter(|(_, fd)| fd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
        .map(|(index, _)| index)
        .collect())
}

enum InputStatus {
    Continue,
    EndOfFile,
}

fn handle_text_input(
    reader: &mut BufReader<File>,
    conf_dir: &Path,
    actions: &[ActionRule],
    map: &[MapRule],
    logger: &mut Logger,
) -> Result<InputStatus, Vec<AppletError>> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => Ok(InputStatus::EndOfFile),
        Ok(_) => {
            let line = line.trim();
            if line.is_empty() {
                return Ok(InputStatus::Continue);
            }
            if let Some(action) = resolve_text_event(line, actions, map) {
                logger.log(line)?;
                dispatch_action(conf_dir, &action)?;
            }
            Ok(InputStatus::Continue)
        }
        Err(err) => Err(vec![AppletError::from_io(APPLET, "reading", None, err)]),
    }
}

fn handle_binary_input(
    file: &mut File,
    conf_dir: &Path,
    actions: &[ActionRule],
    map: &[MapRule],
    logger: &mut Logger,
) -> Result<InputStatus, Vec<AppletError>> {
    let mut event = InputEvent::default();
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(
            (&mut event as *mut InputEvent).cast::<u8>(),
            std::mem::size_of::<InputEvent>(),
        )
    };
    match file.read_exact(bytes) {
        Ok(()) => {
            if !(event.value == 0 || event.value == 1) {
                return Ok(InputStatus::Continue);
            }
            if let Some(action) = resolve_binary_event(event, actions, map) {
                logger.log(&format!(
                    "input {:x}:{:x} {}",
                    event.type_, event.code, event.value
                ))?;
                dispatch_action(conf_dir, &action)?;
            }
            Ok(InputStatus::Continue)
        }
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(InputStatus::EndOfFile),
        Err(err) => Err(vec![AppletError::from_io(APPLET, "reading", None, err)]),
    }
}

fn resolve_text_event(line: &str, actions: &[ActionRule], map: &[MapRule]) -> Option<String> {
    let normalized = trim_text_event_tail(line);
    let desc = map
        .iter()
        .find(|rule| normalized.starts_with(&rule.desc))
        .map(|rule| rule.desc.as_str())
        .unwrap_or(normalized);
    resolve_action(desc, actions)
}

fn resolve_binary_event(event: InputEvent, actions: &[ActionRule], map: &[MapRule]) -> Option<String> {
    let desc = map.iter().find(|rule| {
        rule.event_type == event.type_
            && rule.event_code == event.code
            && rule.event_value == event.value
    })?;
    resolve_action(&desc.desc, actions)
}

fn trim_text_event_tail(line: &str) -> &str {
    let trimmed = line.trim();
    let mut parts = trimmed.rsplitn(2, char::is_whitespace);
    let last = parts.next().unwrap_or(trimmed);
    let prefix = parts.next().unwrap_or("");
    if last.len() == 8 && last.chars().all(|ch| ch.is_ascii_hexdigit()) && !prefix.is_empty() {
        prefix.trim_end()
    } else {
        trimmed
    }
}

fn resolve_action(desc: &str, actions: &[ActionRule]) -> Option<String> {
    actions
        .iter()
        .find(|rule| desc.contains(&rule.key))
        .map(|rule| rule.action.clone())
        .or_else(|| (!desc.contains(' ')).then(|| desc.to_string()))
}

fn dispatch_action(conf_dir: &Path, action: &str) -> Result<(), Vec<AppletError>> {
    let path = conf_dir.join(action);
    let metadata = fs::metadata(&path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "stat", Some(&path.to_string_lossy()), err)])?;
    let mut command = if metadata.is_dir() {
        let mut cmd = Command::new("run-parts");
        cmd.arg(&path);
        cmd
    } else {
        Command::new(&path)
    };
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "spawning", Some(&path.to_string_lossy()), err)])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("handler '{}' exited with {}", path.display(), status),
        )])
    }
}

struct PidFileGuard {
    path: PathBuf,
}

impl PidFileGuard {
    fn create(path: &Path) -> Result<Self, Vec<AppletError>> {
        fs::write(path, format!("{}\n", std::process::id()))
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&path.to_string_lossy()), err)])?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

extern "C" fn handle_signal(_: libc::c_int) {
    RUNNING.store(false, Ordering::Relaxed);
}

struct SignalGuard {
    old_int: libc::sighandler_t,
    old_term: libc::sighandler_t,
}

impl SignalGuard {
    fn install() -> Self {
        RUNNING.store(true, Ordering::Relaxed);
        let handler = handle_signal as *const () as libc::sighandler_t;
        let old_int = unsafe { libc::signal(libc::SIGINT, handler) };
        let old_term = unsafe { libc::signal(libc::SIGTERM, handler) };
        Self { old_int, old_term }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.old_int);
            libc::signal(libc::SIGTERM, self.old_term);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionRule, InputEvent, MapRule, default_action_rules, default_map_rules, read_action_rules,
        read_map_rules, resolve_binary_event, resolve_text_event, trim_text_event_tail,
    };
    use std::fs;

    #[test]
    fn trims_proc_event_status_tail() {
        assert_eq!(
            trim_text_event_tail("button/power PWRF 00000080 00000000"),
            "button/power PWRF 00000080"
        );
    }

    #[test]
    fn resolves_text_events_with_default_mapping() {
        let action = resolve_text_event(
            "button/power PWRF 00000080 00000000",
            &default_action_rules(),
            &default_map_rules(),
        );
        assert_eq!(action.as_deref(), Some("PWRF/00000080"));
    }

    #[test]
    fn resolves_binary_events_with_map_file_rules() {
        let action = resolve_binary_event(
            InputEvent {
                type_: 1,
                code: 116,
                value: 1,
                ..InputEvent::default()
            },
            &[ActionRule {
                key: String::from("PWRF"),
                action: String::from("power"),
            }],
            &[MapRule {
                event_type: 1,
                event_code: 116,
                event_value: 1,
                desc: String::from("button/power PWRF 00000080"),
            }],
        );
        assert_eq!(action.as_deref(), Some("power"));
    }

    #[test]
    fn action_file_missing_uses_defaults() {
        let path = std::env::temp_dir().join(format!("seed-acpid-action-{}", std::process::id()));
        let rules = read_action_rules(&path).unwrap();
        assert!(rules.iter().any(|rule| rule.key == "PWRF"));
    }

    #[test]
    fn map_file_parses_six_columns() {
        let path = std::env::temp_dir().join(format!("seed-acpid-map-{}", std::process::id()));
        fs::write(
            &path,
            "EV_KEY 01 KEY_POWER 116 1 button/power PWRF 00000080\n",
        )
        .unwrap();
        let rules = read_map_rules(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(rules[0].event_type, 1);
        assert_eq!(rules[0].event_code, 116);
        assert_eq!(rules[0].desc, "button/power PWRF 00000080");
    }
}
