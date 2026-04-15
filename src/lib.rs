pub mod common;
#[cfg(feature = "wget")]
pub mod wget;

#[cfg(feature = "multicall")]
#[macro_use]
mod applet_list;
#[cfg(feature = "multicall")]
pub mod applets;

#[cfg(feature = "multicall")]
use std::path::Path;

#[cfg(feature = "multicall")]
type AppletMain = fn(&[String]) -> i32;

#[cfg(feature = "multicall")]
struct AppletEntry {
    name: &'static str,
    main: AppletMain,
}

#[cfg(feature = "multicall")]
const APPLETS: &[AppletEntry] = define_applet_entries!(AppletEntry);

#[cfg(feature = "multicall")]
pub fn dispatch(argv: &[String]) -> i32 {
    let Some(applet) = determine_applet(argv) else {
        eprintln!("seed: missing applet name");
        return 1;
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
    args: &'a [String],
}

#[cfg(feature = "multicall")]
fn determine_applet(argv: &[String]) -> Option<AppletInvocation<'_>> {
    let program_name = argv.first()?;
    let argv0 = Path::new(program_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program_name);

    if argv0 == "busybox" || argv0 == "seed" {
        let applet = argv.get(1)?.as_str();
        return Some(AppletInvocation {
            name: applet,
            args: &argv[2..],
        });
    }

    Some(AppletInvocation {
        name: argv0,
        args: &argv[1..],
    })
}
