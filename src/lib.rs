pub mod applets;
pub mod common;

use std::ffi::CString;
use std::path::Path;

const SIGPIPE: i32 = 13;
const SIG_IGN: usize = 1;

unsafe extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
}

pub fn install_signal_handlers() {
    // SAFETY: `signal` is called with a valid signal number and the process-wide
    // disposition `SIG_IGN` so writes to a closed pipe terminate cleanly.
    unsafe {
        signal(SIGPIPE, SIG_IGN);
    }
}

pub fn dispatch(argv: &[String]) -> i32 {
    let Some(applet) = determine_applet(argv) else {
        eprintln!("seed: missing applet name");
        return 1;
    };

    match applet.name.as_str() {
        "cat" => applets::cat::main(applet.args),
        other => {
            eprintln!("seed: unknown applet: {other}");
            1
        }
    }
}

struct AppletInvocation<'a> {
    name: String,
    args: &'a [String],
}

fn determine_applet(argv: &[String]) -> Option<AppletInvocation<'_>> {
    let program_name = argv.first()?;
    let argv0 = Path::new(program_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program_name);

    if argv0 == "busybox" || argv0 == "seed" {
        let applet = argv.get(1)?;
        return Some(AppletInvocation {
            name: applet.clone(),
            args: &argv[2..],
        });
    }

    Some(AppletInvocation {
        name: argv0.to_owned(),
        args: &argv[1..],
    })
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
