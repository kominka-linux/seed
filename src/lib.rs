pub mod applets;
pub mod common;

use std::path::Path;

pub fn dispatch(argv: &[String]) -> i32 {
    let Some(applet) = determine_applet(argv) else {
        eprintln!("seed: missing applet name");
        return 1;
    };

    match applet.name.as_str() {
        "cat" => applets::cat::main(applet.args),
        "chmod" => applets::chmod::main(applet.args),
        "cp" => applets::cp::main(applet.args),
        "diff" => applets::diff::main(applet.args),
        "egrep" => applets::grep::main_extended(applet.args),
        "find" => applets::find::main(applet.args),
        "grep" => applets::grep::main(applet.args),
        "mkdir" => applets::mkdir::main(applet.args),
        "ls" => applets::ls::main(applet.args),
        "mv" => applets::mv::main(applet.args),
        "od" => applets::od::main(applet.args),
        "rm" => applets::rm::main(applet.args),
        "rmdir" => applets::rmdir::main(applet.args),
        "printf" => applets::printf::main(applet.args),
        "sort" => applets::sort::main(applet.args),
        "tee" => applets::tee::main(applet.args),
        "wc" => applets::wc::main(applet.args),
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
