pub mod common;
#[cfg(feature = "wget")]
pub mod wget;

#[cfg(feature = "multicall")]
#[macro_use]
mod applet_list;
#[cfg(feature = "multicall")]
pub mod applets;

#[cfg(feature = "multicall")]
use std::ffi::OsStr;
#[cfg(feature = "multicall")]
use std::path::Path;

#[cfg(feature = "multicall")]
type AppletMain = fn(&[std::ffi::OsString]) -> i32;

#[cfg(feature = "multicall")]
struct AppletEntry {
    name: &'static str,
    main: AppletMain,
}

#[cfg(feature = "multicall")]
const APPLETS: &[AppletEntry] = define_applet_entries!(AppletEntry);

#[cfg(feature = "multicall")]
pub fn dispatch(argv: &[std::ffi::OsString]) -> i32 {
    let applet = match determine_applet(argv) {
        Ok(applet) => applet,
        Err(message) => {
            eprintln!("seed: {message}");
            return 1;
        }
    };

    if let Some(entry) = APPLETS.iter().find(|entry| entry.name == applet.name) {
        (entry.main)(applet.args)
    } else {
        eprintln!("seed: unknown applet: {}", applet.name);
        1
    }
}

#[cfg(feature = "multicall")]
struct AppletInvocation<'a> {
    name: &'a str,
    args: &'a [std::ffi::OsString],
}

#[cfg(feature = "multicall")]
fn determine_applet(argv: &[std::ffi::OsString]) -> Result<AppletInvocation<'_>, &'static str> {
    let Some(program_name) = argv.first() else {
        return Err("missing applet name");
    };
    let argv0 = Path::new(program_name)
        .file_name()
        .unwrap_or(program_name.as_os_str());

    if argv0 == OsStr::new("busybox") || argv0 == OsStr::new("seed") {
        let Some(applet) = argv.get(1) else {
            return Err("missing applet name");
        };
        let Some(applet) = applet.to_str() else {
            return Err("applet name is invalid unicode");
        };
        return Ok(AppletInvocation {
            name: applet,
            args: &argv[2..],
        });
    }

    Ok(AppletInvocation {
        name: argv0.to_str().ok_or("applet name is invalid unicode")?,
        args: &argv[1..],
    })
}
