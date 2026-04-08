use std::ffi::CString;

pub fn install_signal_handlers() {
    // SAFETY: `signal` is called with a valid signal number and the process-wide
    // disposition `SIG_IGN` so writes to a closed pipe terminate cleanly.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

pub fn argv() -> Vec<String> {
    std::env::args_os()
        .map(|arg| {
            CString::new(arg.as_encoded_bytes())
                .ok()
                .and_then(|cstr| cstr.into_string().ok())
                .unwrap_or_else(|| String::from_utf8_lossy(arg.as_encoded_bytes()).into_owned())
        })
        .collect()
}
