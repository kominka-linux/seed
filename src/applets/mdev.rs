use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "mdev";
const DEFAULT_DEV_ROOT: &str = "/dev";
const DEFAULT_SYSFS_ROOT: &str = "/sys";
const DEFAULT_CONF: &str = "/etc/mdev.conf";
const DEFAULT_FIRMWARE_ROOT: &str = "/lib/firmware";
const DEFAULT_MODE: u32 = 0o660;
const DEFAULT_UID: libc::uid_t = 0;
const DEFAULT_GID: libc::gid_t = 0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Add,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeKind {
    Block,
    Char,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    scan: bool,
    daemon: bool,
    foreground: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Paths {
    dev_root: PathBuf,
    sysfs_root: PathBuf,
    conf: PathBuf,
    firmware_root: PathBuf,
    seq_file: PathBuf,
    plain_files: bool,
}

#[derive(Debug)]
struct Event {
    action: Action,
    device_name: String,
    sysfs_path: PathBuf,
    kind: Option<NodeKind>,
    major: Option<u32>,
    minor: Option<u32>,
    env: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeviceSpec {
    kind: NodeKind,
    major: u32,
    minor: u32,
    mode: u32,
    uid: libc::uid_t,
    gid: libc::gid_t,
}

#[derive(Debug)]
struct Rule {
    keep_matching: bool,
    env_matches: Vec<EnvMatch>,
    pattern: RulePattern,
    uid: libc::uid_t,
    gid: libc::gid_t,
    mode: u32,
    alias: Option<AliasRule>,
    command: Option<CommandRule>,
}

#[derive(Debug)]
struct EnvMatch {
    name: String,
    regex: CompiledRegex,
}

#[derive(Debug)]
enum RulePattern {
    Name(CompiledRegex),
    EnvValue { name: String, regex: CompiledRegex },
    MajorMinor { major: u32, minor_start: u32, minor_end: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AliasRule {
    Rename(String),
    RenameAndLink(String),
    Suppress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandTrigger {
    Add,
    Remove,
    Any,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CommandRule {
    trigger: CommandTrigger,
    command: String,
}

#[derive(Debug)]
struct CompiledRegex {
    regex: libc::regex_t,
}

impl Drop for CompiledRegex {
    fn drop(&mut self) {
        // SAFETY: `self.regex` was initialized by `regcomp` and may be safely
        // released with `regfree` exactly once here.
        unsafe {
            libc::regfree(&mut self.regex);
        }
    }
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    let options = parse_args(args)?;
    let paths = Paths::from_env();
    let rules = read_rules(&paths.conf)?;

    // SAFETY: `umask` updates process-global file creation permissions and
    // takes a plain value.
    unsafe {
        libc::umask(0);
    }

    if options.daemon {
        return run_daemon(&paths, &rules, options.foreground);
    }
    if options.scan {
        return run_scan(&paths, &rules);
    }
    let event = Event::from_process_env(&paths)?;
    handle_event(&paths, &rules, &event)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg(APPLET)? {
        match token {
            ArgToken::LongOption("scan", None) => options.scan = true,
            ArgToken::LongOption("daemon", None) => options.daemon = true,
            ArgToken::LongOption("foreground", None) => options.foreground = true,
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                for flag in flags.chars() {
                    match flag {
                        's' => options.scan = true,
                        'd' => options.daemon = true,
                        'f' => options.foreground = true,
                        _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                    }
                }
            }
            ArgToken::Operand(_) => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }

    if options.foreground && !options.daemon {
        return Err(vec![AppletError::new(
            APPLET,
            "foreground mode requires daemon mode",
        )]);
    }

    Ok(options)
}

impl Paths {
    fn from_env() -> Self {
        let dev_root = env::var_os("SEED_MDEV_DEV_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_DEV_ROOT));
        let seq_file = env::var_os("SEED_MDEV_SEQ")
            .map(PathBuf::from)
            .unwrap_or_else(|| dev_root.join("mdev.seq"));
        Self {
            dev_root,
            sysfs_root: env::var_os("SEED_MDEV_SYSFS")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SYSFS_ROOT)),
            conf: env::var_os("SEED_MDEV_CONF")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_CONF)),
            firmware_root: env::var_os("SEED_MDEV_FIRMWARE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_FIRMWARE_ROOT)),
            seq_file,
            plain_files: env::var_os("SEED_MDEV_TEST_PLAIN_FILES").is_some(),
        }
    }
}

impl Event {
    fn from_process_env(paths: &Paths) -> Result<Self, Vec<AppletError>> {
        let env_map = env::vars().collect::<HashMap<_, _>>();
        Self::from_env_map(paths, env_map)
    }

    fn from_env_map(paths: &Paths, mut env_map: HashMap<String, String>) -> Result<Self, Vec<AppletError>> {
        let action = match env_map.get("ACTION").map(String::as_str) {
            Some("remove") => Action::Remove,
            Some(_) => Action::Add,
            None => return Err(vec![AppletError::new(APPLET, "missing ACTION environment variable")]),
        };
        let Some(devpath) = env_map.get("DEVPATH") else {
            return Err(vec![AppletError::new(
                APPLET,
                "missing DEVPATH environment variable",
            )]);
        };

        let sysfs_path = paths.sysfs_root.join(devpath.trim_start_matches('/'));
        let uevent_env = read_uevent_env(&sysfs_path).unwrap_or_default();
        for (key, value) in uevent_env {
            env_map.entry(key).or_insert(value);
        }

        let subsystem = env_map
            .get("SUBSYSTEM")
            .cloned()
            .or_else(|| read_subsystem(&sysfs_path).ok().flatten());
        if let Some(subsystem_name) = subsystem.as_ref() {
            env_map
                .entry(String::from("SUBSYSTEM"))
                .or_insert_with(|| subsystem_name.clone());
        }

        let device_name = env_map
            .get("DEVNAME")
            .cloned()
            .or_else(|| read_uevent_env(&sysfs_path).ok().and_then(|envs| envs.get("DEVNAME").cloned()))
            .unwrap_or_else(|| {
                sysfs_path
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        env_map
            .entry(String::from("DEVNAME"))
            .or_insert_with(|| device_name.clone());
        env_map
            .entry(String::from("ACTION"))
            .or_insert_with(|| match action {
                Action::Add => String::from("add"),
                Action::Remove => String::from("remove"),
            });

        let (major, minor) = if action == Action::Add {
            read_dev_numbers(&sysfs_path).unwrap_or((None, None))
        } else {
            (None, None)
        };
        let kind = determine_kind(&sysfs_path, subsystem.as_deref());

        Ok(Self {
            action,
            device_name,
            sysfs_path,
            kind,
            major,
            minor,
            env: env_map,
        })
    }
}

fn read_rules(path: &Path) -> Result<Vec<Rule>, Vec<AppletError>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rule = parse_rule_line(line, index + 1)?;
        rules.push(rule);
    }
    Ok(rules)
}

fn parse_rule_line(line: &str, line_number: usize) -> Result<Rule, Vec<AppletError>> {
    let (pattern_field, rest) =
        next_field(line).ok_or_else(|| vec![config_error(line_number, "missing pattern")])?;
    let (owner_field, rest) =
        next_field(rest).ok_or_else(|| vec![config_error(line_number, "missing uid:gid")])?;
    let (mode_field, mut rest) =
        next_field(rest).ok_or_else(|| vec![config_error(line_number, "missing mode")])?;

    let keep_matching = pattern_field.starts_with('-');
    let mut pattern_text = if keep_matching {
        &pattern_field[1..]
    } else {
        pattern_field
    };
    let mut env_matches = Vec::new();
    while let Some((env_match, remaining)) = parse_env_prefix(pattern_text) {
        env_matches.push(env_match);
        pattern_text = remaining;
    }
    if pattern_text.is_empty() {
        return Err(vec![config_error(line_number, "missing match pattern")]);
    }

    let pattern = parse_rule_pattern(pattern_text, line_number)?;
    let (uid, gid) = parse_uid_gid(owner_field, line_number)?;
    let mode = u32::from_str_radix(mode_field, 8)
        .map_err(|_| vec![config_error(line_number, format!("invalid mode '{mode_field}'"))])?;

    rest = rest.trim_start();
    let mut alias = None;
    if let Some(first) = rest.chars().next()
        && matches!(first, '>' | '=' | '!')
    {
        if first == '!' {
            alias = Some(AliasRule::Suppress);
            rest = rest[1..].trim_start();
        } else {
            let (alias_field, remaining) =
                next_field(rest).ok_or_else(|| vec![config_error(line_number, "missing alias target")])?;
            rest = remaining.trim_start();
            alias = Some(match first {
                '>' => AliasRule::RenameAndLink(alias_field[1..].to_string()),
                '=' => AliasRule::Rename(alias_field[1..].to_string()),
                _ => unreachable!(),
            });
        }
    }

    let command = if rest.is_empty() {
        None
    } else {
        let trigger = match rest.chars().next().unwrap_or(' ') {
            '@' => CommandTrigger::Add,
            '$' => CommandTrigger::Remove,
            '*' => CommandTrigger::Any,
            _ => return Err(vec![config_error(line_number, "invalid command action")]),
        };
        let command = rest[1..].trim_start();
        if command.is_empty() {
            return Err(vec![config_error(line_number, "empty command")]);
        }
        Some(CommandRule {
            trigger,
            command: command.to_string(),
        })
    };

    Ok(Rule {
        keep_matching,
        env_matches,
        pattern,
        uid,
        gid,
        mode,
        alias,
        command,
    })
}

fn next_field(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed
        .find(char::is_whitespace)
        .unwrap_or(trimmed.len());
    Some((&trimmed[..end], &trimmed[end..]))
}

fn parse_env_prefix(text: &str) -> Option<(EnvMatch, &str)> {
    let eq = text.find('=')?;
    let env_name = &text[..eq];
    if env_name.is_empty() || !env_name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        return None;
    }
    let remainder = &text[eq + 1..];
    let semi = remainder.find(';')?;
    let regex_text = &remainder[..semi];
    let regex = CompiledRegex::new(regex_text).ok()?;
    Some((
        EnvMatch {
            name: env_name.to_string(),
            regex,
        },
        &remainder[semi + 1..],
    ))
}

fn parse_rule_pattern(text: &str, line_number: usize) -> Result<RulePattern, Vec<AppletError>> {
    if let Some(rest) = text.strip_prefix('@') {
        return parse_major_minor_pattern(rest, line_number);
    }
    if let Some(rest) = text.strip_prefix('$') {
        let (name, regex_text) = rest.split_once('=').ok_or_else(|| {
            vec![config_error(
                line_number,
                format!("invalid env match '{text}'"),
            )]
        })?;
        if name.is_empty() {
            return Err(vec![config_error(line_number, "missing env name")]);
        }
        return Ok(RulePattern::EnvValue {
            name: name.to_string(),
            regex: CompiledRegex::new(regex_text)
                .map_err(|_| vec![config_error(line_number, format!("invalid regex '{regex_text}'"))])?,
        });
    }
    Ok(RulePattern::Name(
        CompiledRegex::new(text)
            .map_err(|_| vec![config_error(line_number, format!("invalid regex '{text}'"))])?,
    ))
}

fn parse_major_minor_pattern(text: &str, line_number: usize) -> Result<RulePattern, Vec<AppletError>> {
    let (major_text, minor_text) = text
        .split_once(',')
        .ok_or_else(|| vec![config_error(line_number, format!("invalid major/minor match '@{text}'"))])?;
    let major = major_text
        .parse::<u32>()
        .map_err(|_| vec![config_error(line_number, format!("invalid major in '@{text}'"))])?;
    let (minor_start, minor_end) = if let Some((start, end)) = minor_text.split_once('-') {
        let start = start
            .parse::<u32>()
            .map_err(|_| vec![config_error(line_number, format!("invalid minor range in '@{text}'"))])?;
        let end = end
            .parse::<u32>()
            .map_err(|_| vec![config_error(line_number, format!("invalid minor range in '@{text}'"))])?;
        (start, end)
    } else {
        let minor = minor_text
            .parse::<u32>()
            .map_err(|_| vec![config_error(line_number, format!("invalid minor in '@{text}'"))])?;
        (minor, minor)
    };
    Ok(RulePattern::MajorMinor {
        major,
        minor_start,
        minor_end,
    })
}

fn parse_uid_gid(spec: &str, line_number: usize) -> Result<(libc::uid_t, libc::gid_t), Vec<AppletError>> {
    let (uid_text, gid_text) = spec
        .split_once(':')
        .ok_or_else(|| vec![config_error(line_number, "uid:gid field is required")])?;
    Ok((
        parse_uid(uid_text)
            .map_err(|message| vec![config_error(line_number, message)])?,
        parse_gid(gid_text)
            .map_err(|message| vec![config_error(line_number, message)])?,
    ))
}

fn parse_uid(text: &str) -> Result<libc::uid_t, String> {
    if let Ok(uid) = text.parse::<libc::uid_t>() {
        return Ok(uid);
    }
    let name = CString::new(text).map_err(|_| format!("invalid user '{text}'"))?;
    // SAFETY: `name` is a valid NUL-terminated user name.
    let entry = unsafe { libc::getpwnam(name.as_ptr()) };
    if entry.is_null() {
        Err(format!("unknown user '{text}'"))
    } else {
        // SAFETY: non-null pointer returned by libc for the duration of this call.
        Ok(unsafe { (*entry).pw_uid })
    }
}

fn parse_gid(text: &str) -> Result<libc::gid_t, String> {
    if let Ok(gid) = text.parse::<libc::gid_t>() {
        return Ok(gid);
    }
    let name = CString::new(text).map_err(|_| format!("invalid group '{text}'"))?;
    // SAFETY: `name` is a valid NUL-terminated group name.
    let entry = unsafe { libc::getgrnam(name.as_ptr()) };
    if entry.is_null() {
        Err(format!("unknown group '{text}'"))
    } else {
        // SAFETY: non-null pointer returned by libc for the duration of this call.
        Ok(unsafe { (*entry).gr_gid })
    }
}

impl CompiledRegex {
    fn new(pattern: &str) -> Result<Self, ()> {
        let c_pattern = CString::new(pattern).map_err(|_| ())?;
        let mut regex = MaybeUninit::<libc::regex_t>::uninit();
        // SAFETY: `regex` points to uninitialized storage owned by this
        // function, and `c_pattern` is a valid NUL-terminated regex string.
        let rc = unsafe { libc::regcomp(regex.as_mut_ptr(), c_pattern.as_ptr(), libc::REG_EXTENDED) };
        if rc != 0 {
            return Err(());
        }
        Ok(Self {
            // SAFETY: `regcomp` initialized `regex` when it returned 0.
            regex: unsafe { regex.assume_init() },
        })
    }

    fn is_full_match(&self, text: &str) -> bool {
        let mut captures = empty_matches();
        self.capture(text, &mut captures)
    }

    fn capture(&self, text: &str, matches: &mut [libc::regmatch_t; 10]) -> bool {
        let Ok(c_text) = CString::new(text) else {
            return false;
        };
        // SAFETY: `self.regex` is a compiled POSIX regex, `c_text` is a valid
        // NUL-terminated input string, and `matches` points to writable storage.
        let rc = unsafe {
            libc::regexec(
                &self.regex,
                c_text.as_ptr(),
                matches.len(),
                matches.as_mut_ptr(),
                0,
            )
        };
        rc == 0
            && matches[0].rm_so == 0
            && usize::try_from(matches[0].rm_eo).ok() == Some(text.len())
    }
}

fn empty_matches() -> [libc::regmatch_t; 10] {
    [libc::regmatch_t { rm_so: -1, rm_eo: -1 }; 10]
}

fn run_scan(paths: &Paths, rules: &[Rule]) -> AppletResult {
    let root = paths.sysfs_root.join("dev");
    let mut stack = vec![root];
    let mut visited = HashSet::new();

    while let Some(path) = stack.pop() {
        let canonical = match fs::canonicalize(&path) {
            Ok(value) => value,
            Err(_) => path.clone(),
        };
        if !visited.insert(canonical) {
            continue;
        }

        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(&path.to_string_lossy()),
                    err,
                )]);
            }
        };

        for entry in entries {
            let entry = entry.map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "reading",
                    Some(&path.to_string_lossy()),
                    err,
                )]
            })?;
            let entry_path = entry.path();
            let metadata = match fs::metadata(&entry_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_dir() {
                stack.push(entry_path);
                continue;
            }
            if entry.file_name() != "dev" {
                continue;
            }
            let Some(device_dir) = entry_path.parent() else {
                continue;
            };
            let event = Event::from_scan_path(paths, device_dir)?;
            handle_event(paths, rules, &event)?;
        }
    }

    Ok(())
}

impl Event {
    fn from_scan_path(paths: &Paths, sysfs_path: &Path) -> Result<Self, Vec<AppletError>> {
        let mut env_map = read_uevent_env(sysfs_path).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&sysfs_path.join("uevent").to_string_lossy()),
                err,
            )]
        })?;
        env_map.insert(String::from("ACTION"), String::from("add"));
        let devpath = format!("/{}", sysfs_path.strip_prefix(&paths.sysfs_root).unwrap_or(sysfs_path).display());
        env_map.insert(String::from("DEVPATH"), devpath);
        if let Some(subsystem) = read_subsystem(sysfs_path)? {
            env_map.insert(String::from("SUBSYSTEM"), subsystem);
        }
        Self::from_env_map(paths, env_map)
    }
}

fn handle_event(paths: &Paths, rules: &[Rule], event: &Event) -> AppletResult {
    let _seq_guard = SeqGuard::acquire(paths, &event.env)?;
    let mut matched = false;

    for rule in rules {
        let mut captures = empty_matches();
        if !rule_matches(rule, event, &mut captures) {
            continue;
        }
        matched = true;
        apply_rule(paths, event, rule, &captures)?;
        if !rule.keep_matching {
            break;
        }
    }

    if !matched {
        let default_rule = Rule {
            keep_matching: false,
            env_matches: Vec::new(),
            pattern: RulePattern::Name(CompiledRegex::new(".*").expect("default regex")),
            uid: DEFAULT_UID,
            gid: DEFAULT_GID,
            mode: DEFAULT_MODE,
            alias: None,
            command: None,
        };
        apply_rule(paths, event, &default_rule, &empty_matches())?;
    }

    if event.action == Action::Add {
        maybe_load_firmware(paths, event)?;
    }

    Ok(())
}

fn rule_matches(rule: &Rule, event: &Event, captures: &mut [libc::regmatch_t; 10]) -> bool {
    if !rule.env_matches.iter().all(|entry| {
        event
            .env
            .get(&entry.name)
            .map(|value| entry.regex.is_full_match(value))
            .unwrap_or(false)
    }) {
        return false;
    }

    match &rule.pattern {
        RulePattern::Name(regex) => regex.capture(&event.device_name, captures),
        RulePattern::EnvValue { name, regex } => event
            .env
            .get(name)
            .map(|value| regex.capture(value, captures))
            .unwrap_or(false),
        RulePattern::MajorMinor {
            major,
            minor_start,
            minor_end,
        } => matches!(
            (event.major, event.minor),
            (Some(event_major), Some(event_minor))
                if event_major == *major && event_minor >= *minor_start && event_minor <= *minor_end
        ),
    }
}

fn apply_rule(
    paths: &Paths,
    event: &Event,
    rule: &Rule,
    captures: &[libc::regmatch_t; 10],
) -> AppletResult {
    let (node_name, create_link, suppressed) = resolve_node_name(rule.alias.as_ref(), &event.device_name, captures);

    if event.action == Action::Add {
        if !suppressed
            && let (Some(kind), Some(major), Some(minor)) = (event.kind, event.major, event.minor)
        {
            let spec = DeviceSpec {
                kind,
                major,
                minor,
                mode: rule.mode,
                uid: rule.uid,
                gid: rule.gid,
            };
            create_device(paths, &node_name, spec)?;
            if create_link {
                create_device_link(paths, &event.device_name, &node_name)?;
            }
        }
    } else if !suppressed {
        if create_link {
            remove_node(paths, &event.device_name)?;
        }
        remove_node(paths, &node_name)?;
    }

    if let Some(command) = rule.command.as_ref() {
        let should_run = matches!(
            (command.trigger, event.action),
            (CommandTrigger::Any, _)
                | (CommandTrigger::Add, Action::Add)
                | (CommandTrigger::Remove, Action::Remove)
        );
        if should_run {
            run_command(command, &event.env, &node_name)?;
        }
    }

    Ok(())
}

fn resolve_node_name(
    alias: Option<&AliasRule>,
    device_name: &str,
    captures: &[libc::regmatch_t; 10],
) -> (String, bool, bool) {
    match alias {
        None => (device_name.to_string(), false, false),
        Some(AliasRule::Suppress) => (device_name.to_string(), false, true),
        Some(AliasRule::Rename(pattern)) => (build_alias(pattern, device_name, captures), false, false),
        Some(AliasRule::RenameAndLink(pattern)) => (build_alias(pattern, device_name, captures), true, false),
    }
}

fn build_alias(pattern: &str, device_name: &str, captures: &[libc::regmatch_t; 10]) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%'
            && let Some(next) = chars.peek().copied()
            && let Some(index) = next.to_digit(10)
        {
            chars.next();
            let capture = &captures[index as usize];
            if capture.rm_so >= 0
                && capture.rm_eo >= capture.rm_so
                && let (Ok(start), Ok(end)) =
                    (usize::try_from(capture.rm_so), usize::try_from(capture.rm_eo))
                && let Some(fragment) = device_name.get(start..end)
            {
                out.push_str(fragment);
            }
            continue;
        }
        out.push(ch);
    }
    if out.ends_with('/') {
        out.push_str(device_name);
    }
    out
}

fn create_device(paths: &Paths, node_name: &str, spec: DeviceSpec) -> AppletResult {
    let path = paths.dev_root.join(node_name);
    ensure_parent_dir(&path)?;

    if paths.plain_files {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "creating", Some(&path.to_string_lossy()), err)])?;
        let kind_char = match spec.kind {
            NodeKind::Block => 'b',
            NodeKind::Char => 'c',
        };
        writeln!(file, "{kind_char} {}:{}", spec.major, spec.minor)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&path.to_string_lossy()), err)])?;
    } else {
        let c_path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| vec![AppletError::new(APPLET, "device path contains NUL byte")])?;
        let type_bits = match spec.kind {
            NodeKind::Block => libc::S_IFBLK,
            NodeKind::Char => libc::S_IFCHR,
        };
        let raw_mode = libc::mode_t::try_from(type_bits | spec.mode)
            .map_err(|_| vec![AppletError::new(APPLET, "mode out of range")])?;
        let dev = libc::makedev(spec.major, spec.minor);
        // SAFETY: `c_path` is a valid NUL-terminated path, and the remaining
        // arguments are plain values forwarded to `mknod`.
        let rc = unsafe { libc::mknod(c_path.as_ptr(), raw_mode, dev) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(vec![AppletError::from_io(
                    APPLET,
                    "creating",
                    Some(&path.to_string_lossy()),
                    err,
                )]);
            }
        }
    }

    fs::set_permissions(&path, fs::Permissions::from_mode(spec.mode))
        .map_err(|err| vec![AppletError::from_io(APPLET, "chmod", Some(&path.to_string_lossy()), err)])?;
    chown_path(&path, spec.uid, spec.gid)?;
    Ok(())
}

fn create_device_link(paths: &Paths, device_name: &str, node_name: &str) -> AppletResult {
    let link_path = paths.dev_root.join(device_name);
    ensure_parent_dir(&link_path)?;
    remove_existing(&link_path)?;
    symlink(node_name, &link_path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "symlink", Some(&link_path.to_string_lossy()), err)])
}

fn remove_node(paths: &Paths, node_name: &str) -> AppletResult {
    let path = paths.dev_root.join(node_name);
    remove_existing(&path)
}

fn ensure_parent_dir(path: &Path) -> AppletResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "creating",
                Some(&parent.to_string_lossy()),
                err,
            )]
        })?;
    }
    Ok(())
}

fn remove_existing(path: &Path) -> AppletResult {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_dir() {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("refusing to remove directory {}", path.display()),
                )]);
            }
            fs::remove_file(path).map_err(|err| {
                vec![AppletError::from_io(
                    APPLET,
                    "removing",
                    Some(&path.to_string_lossy()),
                    err,
                )]
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(vec![AppletError::from_io(
            APPLET,
            "stat",
            Some(&path.to_string_lossy()),
            err,
        )]),
    }
}

fn chown_path(path: &Path, uid: libc::uid_t, gid: libc::gid_t) -> AppletResult {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| vec![AppletError::new(APPLET, "device path contains NUL byte")])?;
    // SAFETY: `c_path` is a valid NUL-terminated path and `uid`/`gid` are plain values.
    let rc = unsafe { libc::chown(c_path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::from_io(
            APPLET,
            "chown",
            Some(&path.to_string_lossy()),
            std::io::Error::last_os_error(),
        )])
    }
}

fn run_command(command: &CommandRule, env_map: &HashMap<String, String>, node_name: &str) -> AppletResult {
    let status = Command::new("sh")
        .arg("-c")
        .arg(&command.command)
        .envs(env_map.iter())
        .env("MDEV", node_name)
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "running", None, err)])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("command failed: {}", command.command),
        )])
    }
}

fn maybe_load_firmware(paths: &Paths, event: &Event) -> AppletResult {
    let Some(firmware_name) = event.env.get("FIRMWARE") else {
        return Ok(());
    };
    let firmware_path = paths.firmware_root.join(firmware_name);
    let loading_path = event.sysfs_path.join("loading");
    let data_path = event.sysfs_path.join("data");
    let deadline = Instant::now() + Duration::from_secs(30);

    while !loading_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
    }

    let mut loading = OpenOptions::new()
        .write(true)
        .open(&loading_path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&loading_path.to_string_lossy()), err)])?;
    let mut ok = false;

    if let Ok(mut firmware) = File::open(&firmware_path) {
        loading
            .write_all(b"1")
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&loading_path.to_string_lossy()), err)])?;
        let mut data = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&data_path)
            .map_err(|err| vec![AppletError::from_io(APPLET, "opening", Some(&data_path.to_string_lossy()), err)])?;
        let mut buffer = Vec::new();
        firmware
            .read_to_end(&mut buffer)
            .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&firmware_path.to_string_lossy()), err)])?;
        data.write_all(&buffer)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&data_path.to_string_lossy()), err)])?;
        ok = true;
    }

    let completion = if ok { b"0".as_slice() } else { b"-1".as_slice() };
    loading
        .write_all(completion)
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing", Some(&loading_path.to_string_lossy()), err)])?;
    Ok(())
}

fn read_uevent_env(sysfs_path: &Path) -> Result<HashMap<String, String>, std::io::Error> {
    let text = fs::read_to_string(sysfs_path.join("uevent"))?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect())
}

fn read_subsystem(sysfs_path: &Path) -> Result<Option<String>, Vec<AppletError>> {
    let path = sysfs_path.join("subsystem");
    let target = match fs::read_link(&path) {
        Ok(target) => target,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.to_string_lossy()),
                err,
            )]);
        }
    };
    Ok(target.file_name().map(|value| value.to_string_lossy().into_owned()))
}

fn read_dev_numbers(sysfs_path: &Path) -> Result<(Option<u32>, Option<u32>), Vec<AppletError>> {
    let path = sysfs_path.join("dev");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        Err(err) => {
            return Err(vec![AppletError::from_io(
                APPLET,
                "reading",
                Some(&path.to_string_lossy()),
                err,
            )]);
        }
    };
    let value = text.trim();
    let Some((major, minor)) = value.split_once(':') else {
        return Ok((None, None));
    };
    Ok((major.parse::<u32>().ok(), minor.parse::<u32>().ok()))
}

fn determine_kind(sysfs_path: &Path, subsystem: Option<&str>) -> Option<NodeKind> {
    if sysfs_path.components().any(|component| component.as_os_str() == "block")
        || matches!(subsystem, Some("block"))
    {
        Some(NodeKind::Block)
    } else {
        Some(NodeKind::Char)
    }
}

struct SeqGuard<'a> {
    path: &'a Path,
    next_value: Option<u64>,
}

impl<'a> SeqGuard<'a> {
    fn acquire(paths: &'a Paths, env_map: &HashMap<String, String>) -> Result<Self, Vec<AppletError>> {
        let Some(seq_text) = env_map.get("SEQNUM") else {
            return Ok(Self {
                path: &paths.seq_file,
                next_value: None,
            });
        };
        if !paths.seq_file.exists() {
            return Ok(Self {
                path: &paths.seq_file,
                next_value: None,
            });
        }
        let expected = seq_text
            .parse::<u64>()
            .map_err(|_| vec![AppletError::new(APPLET, format!("invalid SEQNUM '{seq_text}'"))])?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut current = String::new();
            if let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&paths.seq_file) {
                file.read_to_string(&mut current).map_err(|err| {
                    vec![AppletError::from_io(
                        APPLET,
                        "reading",
                        Some(&paths.seq_file.to_string_lossy()),
                        err,
                    )]
                })?;
                let trimmed = current.trim();
                if trimmed.is_empty() {
                    file.set_len(0).map_err(|err| {
                        vec![AppletError::from_io(
                            APPLET,
                            "writing",
                            Some(&paths.seq_file.to_string_lossy()),
                            err,
                        )]
                    })?;
                    file.write_all(expected.to_string().as_bytes()).map_err(|err| {
                        vec![AppletError::from_io(
                            APPLET,
                            "writing",
                            Some(&paths.seq_file.to_string_lossy()),
                            err,
                        )]
                    })?;
                    return Ok(Self {
                        path: &paths.seq_file,
                        next_value: Some(expected + 1),
                    });
                }
                let seen = trimmed.parse::<u64>().unwrap_or(expected);
                if seen >= expected {
                    return Ok(Self {
                        path: &paths.seq_file,
                        next_value: Some(expected + 1),
                    });
                }
            }
            if Instant::now() >= deadline {
                return Ok(Self {
                    path: &paths.seq_file,
                    next_value: Some(expected + 1),
                });
            }
            thread::sleep(Duration::from_millis(32));
        }
    }
}

impl Drop for SeqGuard<'_> {
    fn drop(&mut self) {
        let Some(next_value) = self.next_value else {
            return;
        };
        if let Ok(mut file) = OpenOptions::new().write(true).truncate(true).open(self.path) {
            let _ = writeln!(file, "{next_value}");
        }
    }
}

fn run_daemon(paths: &Paths, rules: &[Rule], foreground: bool) -> AppletResult {
    let fd = open_uevent_socket()?;
    run_scan(paths, rules)?;
    if !foreground {
        // SAFETY: `daemon` takes plain value flags and detaches the current process.
        let rc = unsafe { libc::daemon(0, 1) };
        if rc != 0 {
            return Err(vec![AppletError::from_io(
                APPLET,
                "daemonizing",
                None,
                std::io::Error::last_os_error(),
            )]);
        }
    }
    daemon_loop(fd, paths, rules)
}

fn open_uevent_socket() -> Result<libc::c_int, Vec<AppletError>> {
    // SAFETY: `socket` receives plain value arguments and returns a file descriptor.
    let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM, libc::NETLINK_KOBJECT_UEVENT) };
    if fd < 0 {
        return Err(vec![AppletError::from_io(
            APPLET,
            "socket",
            None,
            std::io::Error::last_os_error(),
        )]);
    }

    let rcvbuf = 2 * 1024 * 1024;
    // SAFETY: `fd` is a valid socket descriptor and the buffer pointer/length
    // are correct for the `SO_RCVBUF` socket option.
    unsafe {
        let _ = libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            (&rcvbuf as *const i32).cast(),
            std::mem::size_of_val(&rcvbuf) as libc::socklen_t,
        );
    }

    // SAFETY: zeroed initialization is valid for `sockaddr_nl`.
    let mut addr = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
    addr.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    addr.nl_pid = 0;
    addr.nl_groups = 1;
    // SAFETY: `addr` is a valid initialized netlink address structure.
    let rc = unsafe {
        libc::bind(
            fd,
            (&addr as *const libc::sockaddr_nl).cast(),
            std::mem::size_of_val(&addr) as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(fd)
    } else {
        // SAFETY: `fd` is an open descriptor that we no longer need on failure.
        unsafe {
            libc::close(fd);
        }
        Err(vec![AppletError::from_io(
            APPLET,
            "bind",
            None,
            std::io::Error::last_os_error(),
        )])
    }
}

fn daemon_loop(fd: libc::c_int, paths: &Paths, rules: &[Rule]) -> AppletResult {
    let mut buffer = [0u8; 4096];
    loop {
        // SAFETY: `buffer` is valid writable storage for the kernel to fill.
        let len = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if len < 0 {
            return Err(vec![AppletError::from_io(
                APPLET,
                "recv",
                None,
                std::io::Error::last_os_error(),
            )]);
        }
        let len = usize::try_from(len).map_err(|_| vec![AppletError::new(APPLET, "invalid uevent length")])?;
        let env_map = parse_uevent_message(&buffer[..len]);
        if env_map.contains_key("ACTION") && env_map.contains_key("DEVPATH") {
            let event = Event::from_env_map(paths, env_map)?;
            handle_event(paths, rules, &event)?;
        }
    }
}

fn parse_uevent_message(buffer: &[u8]) -> HashMap<String, String> {
    let mut env_map = HashMap::new();
    for (index, field) in buffer.split(|byte| *byte == 0).filter(|field| !field.is_empty()).enumerate() {
        let text = String::from_utf8_lossy(field);
        if let Some((key, value)) = text.split_once('=') {
            env_map.insert(key.to_string(), value.to_string());
        } else if index == 0 && let Some((action, devpath)) = text.split_once('@') {
            env_map.insert(String::from("ACTION"), action.to_string());
            env_map.insert(String::from("DEVPATH"), devpath.to_string());
        }
    }
    env_map
}

fn config_error(line_number: usize, message: impl Into<String>) -> AppletError {
    AppletError::new(APPLET, format!("mdev.conf:{line_number}: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::{
        AliasRule, CommandTrigger, RulePattern, build_alias, empty_matches, parse_rule_line,
        parse_uevent_message, rule_matches,
    };
    use std::collections::HashMap;

    use crate::applets::mdev::Event;

    fn event_with_name(name: &str) -> Event {
        Event {
            action: super::Action::Add,
            device_name: name.to_string(),
            sysfs_path: "/sys/devices/test".into(),
            kind: Some(super::NodeKind::Block),
            major: Some(8),
            minor: Some(1),
            env: HashMap::from([
                (String::from("ACTION"), String::from("add")),
                (String::from("DEVNAME"), name.to_string()),
                (String::from("SUBSYSTEM"), String::from("block")),
            ]),
        }
    }

    #[test]
    fn parses_rule_with_alias_and_command() {
        let rule = parse_rule_line(
            "-SUBSYSTEM=block;(sd[a-z]) 0:6 660 >disk/%1 @echo add",
            1,
        )
        .expect("parse mdev rule");

        assert!(rule.keep_matching);
        assert_eq!(rule.uid, 0);
        assert_eq!(rule.mode, 0o660);
        assert_eq!(rule.alias, Some(AliasRule::RenameAndLink(String::from("disk/%1"))));
        assert_eq!(
            rule.command.as_ref().map(|command| command.trigger),
            Some(CommandTrigger::Add)
        );
        assert!(matches!(rule.pattern, RulePattern::Name(_)));
        assert_eq!(rule.env_matches.len(), 1);
    }

    #[test]
    fn alias_substitutes_capture_groups() {
        let rule = parse_rule_line("(sd[a-z]) 0:0 660 =by-name/%1", 1).expect("parse rule");
        let mut captures = empty_matches();
        assert!(rule_matches(&rule, &event_with_name("sda"), &mut captures));
        assert_eq!(build_alias("by-name/%1", "sda", &captures), "by-name/sda");
    }

    #[test]
    fn parses_uevent_messages_with_header_record() {
        let env = parse_uevent_message(b"add@/devices/virtual/block/loop0\0ACTION=add\0DEVNAME=loop0\0");
        assert_eq!(env.get("ACTION").map(String::as_str), Some("add"));
        assert_eq!(
            env.get("DEVPATH").map(String::as_str),
            Some("/devices/virtual/block/loop0")
        );
        assert_eq!(env.get("DEVNAME").map(String::as_str), Some("loop0"));
    }
}
