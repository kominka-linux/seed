use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::error::AppletError;

const APPLET: &str = "init";
const DEFAULT_INITTAB: &str = "/etc/inittab";
const DEFAULT_SHELL: &str = "/bin/sh";
const RESPAWN_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MIN_RESPAWN_INTERVAL: Duration = Duration::from_secs(1);
const CHILD_STOP_TIMEOUT: Duration = Duration::from_secs(2);

static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    inittab_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Inittab {
    sysinit: Vec<InittabEntry>,
    restart: Vec<InittabEntry>,
    shutdown: Vec<InittabEntry>,
    respawn: Vec<InittabEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InittabEntry {
    id: Option<String>,
    command: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Action {
    Sysinit,
    Restart,
    Shutdown,
    Respawn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SignalAction {
    Restart,
    Halt,
    Poweroff,
    Reboot,
}

#[derive(Debug)]
struct RespawnProcess {
    entry: InittabEntry,
    pid: Option<libc::pid_t>,
    launches: usize,
    next_spawn_at: Option<Instant>,
}

struct SignalGuard {
    previous_hup: libc::sighandler_t,
    previous_term: libc::sighandler_t,
    previous_usr1: libc::sighandler_t,
    previous_usr2: libc::sighandler_t,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    let options = parse_args(args)?;
    ensure_pid1()?;
    let inittab = read_inittab(&options.inittab_path)?;
    let _signals = SignalGuard::install()?;
    run_init(inittab)
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    if !args.is_empty() {
        return Err(vec![AppletError::new(APPLET, "extra operand")]);
    }
    Ok(Options {
        inittab_path: env::var_os("SEED_INIT_INITTAB")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_INITTAB)),
    })
}

fn ensure_pid1() -> Result<(), Vec<AppletError>> {
    if current_pid() == 1 || env_flag("SEED_INIT_TEST_ALLOW_NON_PID1") {
        Ok(())
    } else {
        Err(vec![AppletError::new(APPLET, "PID must be 1")])
    }
}

fn read_inittab(path: &Path) -> Result<Inittab, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    let text = fs::read_to_string(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])?;
    parse_inittab(&text)
}

fn parse_inittab(text: &str) -> Result<Inittab, Vec<AppletError>> {
    let mut table = Inittab {
        sysinit: Vec::new(),
        restart: Vec::new(),
        shutdown: Vec::new(),
        respawn: Vec::new(),
    };
    let mut errors = Vec::new();

    for (line_number, raw_line) in text.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match parse_inittab_line(line) {
            Ok((Action::Sysinit, entry)) => table.sysinit.push(entry),
            Ok((Action::Restart, entry)) => table.restart.push(entry),
            Ok((Action::Shutdown, entry)) => table.shutdown.push(entry),
            Ok((Action::Respawn, entry)) => table.respawn.push(entry),
            Err(message) => errors.push(AppletError::new(
                APPLET,
                format!("invalid inittab line {}: {message}", line_number + 1),
            )),
        }
    }

    if errors.is_empty() {
        Ok(table)
    } else {
        Err(errors)
    }
}

fn parse_inittab_line(line: &str) -> Result<(Action, InittabEntry), String> {
    let mut fields = line.splitn(4, ':');
    let id = fields.next().ok_or_else(|| String::from("missing id"))?;
    let _runlevels = fields
        .next()
        .ok_or_else(|| String::from("missing runlevels"))?;
    let action = fields
        .next()
        .ok_or_else(|| String::from("missing action"))?;
    let command = fields
        .next()
        .ok_or_else(|| String::from("missing command"))?
        .trim();
    if command.is_empty() {
        return Err(String::from("missing command"));
    }

    let action = match action {
        "sysinit" => Action::Sysinit,
        "restart" => Action::Restart,
        "shutdown" => Action::Shutdown,
        "respawn" => Action::Respawn,
        other => return Err(format!("unsupported action '{other}'")),
    };
    let id = (!id.is_empty()).then(|| id.to_string());
    Ok((
        action,
        InittabEntry {
            id,
            command: command.to_string(),
        },
    ))
}

fn run_init(inittab: Inittab) -> AppletCodeResult {
    for entry in &inittab.sysinit {
        run_foreground_entry(entry)?;
    }

    let mut respawns = inittab
        .respawn
        .clone()
        .into_iter()
        .map(|entry| RespawnProcess {
            entry,
            pid: None,
            launches: 0,
            next_spawn_at: None,
        })
        .collect::<Vec<_>>();
    spawn_ready_respawns(&mut respawns)?;

    loop {
        if let Some(signal_action) = take_pending_signal() {
            return handle_signal_action(signal_action, &inittab, &mut respawns);
        }

        reap_children(&mut respawns)?;
        spawn_ready_respawns(&mut respawns)?;
        if should_exit_when_idle(&respawns) {
            return Ok(0);
        }

        thread::sleep(RESPAWN_POLL_INTERVAL);
    }
}

fn spawn_respawn(process: &mut RespawnProcess) -> Result<(), Vec<AppletError>> {
    if process.pid.is_some() || !has_respawns_remaining(process) || !respawn_ready(process) {
        return Ok(());
    }
    let child = spawn_entry_command(&process.entry)
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", Some(&process.entry.command), err)])?;
    let now = Instant::now();
    process.pid = Some(child.id() as libc::pid_t);
    process.launches += 1;
    process.next_spawn_at = Some(now + respawn_delay());
    Ok(())
}

fn spawn_ready_respawns(processes: &mut [RespawnProcess]) -> Result<(), Vec<AppletError>> {
    for process in processes {
        spawn_respawn(process)?;
    }
    Ok(())
}

fn reap_children(processes: &mut [RespawnProcess]) -> Result<(), Vec<AppletError>> {
    loop {
        let mut status = 0;
        // SAFETY: waiting for any child with WNOHANG is valid in the init loop.
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid > 0 {
            mark_reaped_child(processes, pid);
            continue;
        }
        if pid == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ECHILD) {
            return Ok(());
        }
        return Err(vec![AppletError::from_io(
            APPLET,
            "waiting",
            None,
            err,
        )]);
    }
}

fn mark_reaped_child(processes: &mut [RespawnProcess], pid: libc::pid_t) {
    for process in processes {
        if process.pid == Some(pid) {
            process.pid = None;
            return;
        }
    }
}

fn handle_signal_action(
    action: SignalAction,
    inittab: &Inittab,
    respawns: &mut [RespawnProcess],
) -> AppletCodeResult {
    stop_respawn_children(respawns)?;
    match action {
        SignalAction::Restart => run_restart_entries(&inittab.restart),
        SignalAction::Halt => run_shutdown_entries(&inittab.shutdown, "halt"),
        SignalAction::Poweroff => run_shutdown_entries(&inittab.shutdown, "poweroff"),
        SignalAction::Reboot => run_shutdown_entries(&inittab.shutdown, "reboot"),
    }
}

fn run_restart_entries(entries: &[InittabEntry]) -> AppletCodeResult {
    if entries.is_empty() {
        return Ok(0);
    }
    let entry = &entries[0];
    let mut command = base_command(entry);
    let err = command.exec();
    Err(vec![AppletError::new(
        APPLET,
        format!("can't execute '{}': {err}", entry.command),
    )])
}

fn run_shutdown_entries(entries: &[InittabEntry], final_action: &str) -> AppletCodeResult {
    for entry in entries {
        run_foreground_entry(entry)?;
    }
    perform_shutdown(final_action)?;
    Ok(0)
}

fn run_foreground_entry(entry: &InittabEntry) -> Result<(), Vec<AppletError>> {
    let status = base_command(entry)
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", Some(&entry.command), err)])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("command failed: {}", entry.command),
        )])
    }
}

fn stop_respawn_children(processes: &mut [RespawnProcess]) -> Result<(), Vec<AppletError>> {
    for process in processes.iter_mut() {
        if let Some(pid) = process.pid {
            // SAFETY: `pid` is an active child process group leader.
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
        }
    }

    let deadline = Instant::now() + CHILD_STOP_TIMEOUT;
    loop {
        reap_children(processes)?;
        let any_running = processes.iter().any(|process| process.pid.is_some());
        if !any_running {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(RESPAWN_POLL_INTERVAL);
    }

    for process in processes.iter_mut() {
        if let Some(pid) = process.pid {
            // SAFETY: `pid` is an active child process group leader.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    reap_children(processes)?;
    for process in processes.iter_mut() {
        process.pid = None;
    }

    Ok(())
}

fn perform_shutdown(action: &str) -> Result<(), Vec<AppletError>> {
    if let Some(path) = env::var_os("SEED_INIT_TEST_ACTION_LOG") {
        fs::write(path, format!("{action}\n"))
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing", None, err)])?;
        return Ok(());
    }

    // SAFETY: sync() has no failure mode.
    unsafe { libc::sync() };
    let command = match action {
        "halt" => libc::LINUX_REBOOT_CMD_HALT,
        "poweroff" => libc::LINUX_REBOOT_CMD_POWER_OFF,
        "reboot" => libc::LINUX_REBOOT_CMD_RESTART,
        _ => return Err(vec![AppletError::new(APPLET, "invalid shutdown action")]),
    };
    // SAFETY: reboot is called with a valid Linux command constant.
    let rc = unsafe { libc::reboot(command) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("requesting {action}: {}", io::Error::last_os_error()),
        )])
    }
}

fn should_exit_when_idle(processes: &[RespawnProcess]) -> bool {
    env_flag("SEED_INIT_TEST_EXIT_WHEN_IDLE")
        && processes
            .iter()
            .all(|process| process.pid.is_none() && !has_respawns_remaining(process))
}

fn has_respawns_remaining(process: &RespawnProcess) -> bool {
    let Some(limit) = max_respawn_launches() else {
        return true;
    };
    process.launches < limit
}

fn respawn_ready(process: &RespawnProcess) -> bool {
    process
        .next_spawn_at
        .is_none_or(|deadline| Instant::now() >= deadline)
}

fn max_respawn_launches() -> Option<usize> {
    env::var("SEED_INIT_TEST_MAX_RESPAWNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
}

fn respawn_delay() -> Duration {
    env::var("SEED_INIT_TEST_RESPAWN_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(MIN_RESPAWN_INTERVAL)
}

fn spawn_entry_command(entry: &InittabEntry) -> io::Result<Child> {
    let mut command = base_command(entry);
    let tty = entry.id.clone();
    // SAFETY: pre_exec only uses async-signal-safe libc calls plus path opening.
    unsafe {
        command.pre_exec(move || {
            if let Some(tty) = &tty {
                configure_controlling_tty(tty)?;
            } else {
                create_process_group()?;
            }
            Ok(())
        });
    }
    command.spawn()
}

fn base_command(entry: &InittabEntry) -> Command {
    let mut command = Command::new(DEFAULT_SHELL);
    command.arg("-c").arg(&entry.command);
    command
}

fn configure_controlling_tty(id: &str) -> io::Result<()> {
    let tty_path = tty_path(id);
    let c_path = CString::new(tty_path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "tty path contains NUL byte"))?;
    // SAFETY: these libc calls run in the child pre-exec path and use valid
    // descriptors / pointers derived from owned Rust values.
    let sid = unsafe { libc::setsid() };
    if sid < 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::EPERM) {
            return Err(err);
        }
    }
    // SAFETY: `c_path` is a valid NUL-terminated path.
    let fd = unsafe { libc::open(c_path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::dup2(fd, libc::STDIN_FILENO) } < 0
        || unsafe { libc::dup2(fd, libc::STDOUT_FILENO) } < 0
        || unsafe { libc::dup2(fd, libc::STDERR_FILENO) } < 0
    {
        let err = io::Error::last_os_error();
        // SAFETY: `fd` was returned by open above.
        unsafe { libc::close(fd) };
        return Err(err);
    }
    if unsafe { libc::isatty(fd) } == 1 && unsafe { libc::ioctl(fd, libc::TIOCSCTTY as _, 0) } != 0 {
        let err = io::Error::last_os_error();
        // SAFETY: `fd` was returned by open above.
        unsafe { libc::close(fd) };
        return Err(err);
    }
    if fd > libc::STDERR_FILENO {
        // SAFETY: `fd` was returned by open above.
        unsafe { libc::close(fd) };
    }
    Ok(())
}

fn create_process_group() -> io::Result<()> {
    // SAFETY: setpgid(0, 0) makes the child its own process group leader.
    if unsafe { libc::setpgid(0, 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn tty_path(id: &str) -> PathBuf {
    if id.contains('/') {
        return PathBuf::from(id);
    }
    env::var_os("SEED_INIT_TTY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev"))
        .join(id)
}

fn current_pid() -> libc::pid_t {
    // SAFETY: getpid has no preconditions.
    unsafe { libc::getpid() }
}

fn env_flag(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| value != "0")
}

extern "C" fn signal_handler(signal: libc::c_int) {
    PENDING_SIGNAL.store(signal, Ordering::Relaxed);
}

fn take_pending_signal() -> Option<SignalAction> {
    match PENDING_SIGNAL.swap(0, Ordering::Relaxed) {
        libc::SIGHUP => Some(SignalAction::Restart),
        libc::SIGUSR1 => Some(SignalAction::Halt),
        libc::SIGUSR2 => Some(SignalAction::Poweroff),
        libc::SIGTERM => Some(SignalAction::Reboot),
        _ => None,
    }
}

impl SignalGuard {
    fn install() -> Result<Self, Vec<AppletError>> {
        Ok(Self {
            previous_hup: install_signal(libc::SIGHUP)?,
            previous_term: install_signal(libc::SIGTERM)?,
            previous_usr1: install_signal(libc::SIGUSR1)?,
            previous_usr2: install_signal(libc::SIGUSR2)?,
        })
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        // SAFETY: restoring previous signal handlers is safe for the process.
        unsafe {
            libc::signal(libc::SIGHUP, self.previous_hup);
            libc::signal(libc::SIGTERM, self.previous_term);
            libc::signal(libc::SIGUSR1, self.previous_usr1);
            libc::signal(libc::SIGUSR2, self.previous_usr2);
        }
    }
}

fn install_signal(signal: libc::c_int) -> Result<libc::sighandler_t, Vec<AppletError>> {
    // SAFETY: installs a process signal handler with a valid signal number.
    let previous = unsafe { libc::signal(signal, signal_handler as *const () as libc::sighandler_t) };
    if previous == libc::SIG_ERR {
        Err(vec![AppletError::from_io(
            APPLET,
            "installing signal handler",
            None,
            io::Error::last_os_error(),
        )])
    } else {
        Ok(previous)
    }
}

#[cfg(test)]
mod tests {
    use super::{Action, RespawnProcess, has_respawns_remaining, parse_args, parse_inittab, parse_inittab_line, respawn_delay, tty_path};
    use crate::common::test_env;
    use std::time::Duration;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_empty_arguments() {
        let options = parse_args(&[]).unwrap();
        assert!(options.inittab_path.ends_with("/etc/inittab"));
    }

    #[test]
    fn parses_inittab_actions() {
        let (action, entry) = parse_inittab_line("tty1::respawn:/bin/getty 38400 tty1").unwrap();
        assert_eq!(action, Action::Respawn);
        assert_eq!(entry.id.as_deref(), Some("tty1"));
        assert_eq!(entry.command, "/bin/getty 38400 tty1");
    }

    #[test]
    fn parses_full_inittab() {
        let table = parse_inittab(
            "::sysinit:/usr/lib/init/rc.boot\n::restart:/sbin/init\n::shutdown:/usr/lib/init/rc.shutdown\ntty1::respawn:/bin/getty 38400 tty1\n",
        )
        .unwrap();
        assert_eq!(table.sysinit.len(), 1);
        assert_eq!(table.restart.len(), 1);
        assert_eq!(table.shutdown.len(), 1);
        assert_eq!(table.respawn.len(), 1);
    }

    #[test]
    fn rejects_unknown_action() {
        assert!(parse_inittab("::once:/bin/true\n").is_err());
    }

    #[test]
    fn resolves_tty_relative_to_dir() {
        let _guard = test_env::lock();
        unsafe {
            std::env::set_var("SEED_INIT_TTY_DIR", "/tmp/seed-init-tty");
        }
        assert_eq!(
            tty_path("tty1"),
            std::path::PathBuf::from("/tmp/seed-init-tty/tty1")
        );
        unsafe {
            std::env::remove_var("SEED_INIT_TTY_DIR");
        }
    }

    #[test]
    fn extra_operand_errors() {
        assert!(parse_args(&args(&["extra"])).is_err());
    }

    #[test]
    fn respawn_delay_can_be_overridden_for_tests() {
        let _guard = test_env::lock();
        unsafe {
            std::env::set_var("SEED_INIT_TEST_RESPAWN_DELAY_MS", "25");
        }
        assert_eq!(respawn_delay(), Duration::from_millis(25));
        unsafe {
            std::env::remove_var("SEED_INIT_TEST_RESPAWN_DELAY_MS");
        }
    }

    #[test]
    fn respawn_limit_is_respected() {
        let _guard = test_env::lock();
        unsafe {
            std::env::set_var("SEED_INIT_TEST_MAX_RESPAWNS", "2");
        }
        let mut process = RespawnProcess {
            entry: super::InittabEntry {
                id: None,
                command: String::from("true"),
            },
            pid: None,
            launches: 1,
            next_spawn_at: None,
        };
        assert!(has_respawns_remaining(&process));
        process.launches = 2;
        assert!(!has_respawns_remaining(&process));
        unsafe {
            std::env::remove_var("SEED_INIT_TEST_MAX_RESPAWNS");
        }
    }
}
