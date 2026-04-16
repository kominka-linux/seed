use std::ffi::OsString;

pub fn install_signal_handlers() {
    // SAFETY: `signal` is called with a valid signal number and the process-wide
    // disposition `SIG_IGN` so writes to a closed pipe terminate cleanly.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

pub fn argv() -> Vec<OsString> {
    std::env::args_os().collect()
}
