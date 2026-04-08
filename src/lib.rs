pub mod applets;
pub mod common;

use std::path::Path;

type AppletMain = fn(&[String]) -> i32;

struct AppletEntry {
    name: &'static str,
    main: AppletMain,
}

const APPLETS: &[AppletEntry] = &[
    AppletEntry {
        name: "bunzip2",
        main: applets::bzip2::main_bunzip2,
    },
    AppletEntry {
        name: "bzip2",
        main: applets::bzip2::main,
    },
    AppletEntry {
        name: "bzcat",
        main: applets::bzip2::main_bzcat,
    },
    AppletEntry {
        name: "cat",
        main: applets::cat::main,
    },
    AppletEntry {
        name: "chmod",
        main: applets::chmod::main,
    },
    AppletEntry {
        name: "cp",
        main: applets::cp::main,
    },
    AppletEntry {
        name: "date",
        main: applets::date::main,
    },
    AppletEntry {
        name: "diff",
        main: applets::diff::main,
    },
    AppletEntry {
        name: "env",
        main: applets::env::main,
    },
    AppletEntry {
        name: "egrep",
        main: applets::grep::main_extended,
    },
    AppletEntry {
        name: "find",
        main: applets::find::main,
    },
    AppletEntry {
        name: "grep",
        main: applets::grep::main,
    },
    AppletEntry {
        name: "gunzip",
        main: applets::gzip::main_gunzip,
    },
    AppletEntry {
        name: "gzip",
        main: applets::gzip::main,
    },
    AppletEntry {
        name: "ls",
        main: applets::ls::main,
    },
    AppletEntry {
        name: "lzcat",
        main: applets::xz::main_lzcat,
    },
    AppletEntry {
        name: "lzma",
        main: applets::xz::main_lzma,
    },
    AppletEntry {
        name: "mkdir",
        main: applets::mkdir::main,
    },
    AppletEntry {
        name: "mv",
        main: applets::mv::main,
    },
    AppletEntry {
        name: "od",
        main: applets::od::main,
    },
    AppletEntry {
        name: "printf",
        main: applets::printf::main,
    },
    AppletEntry {
        name: "rm",
        main: applets::rm::main,
    },
    AppletEntry {
        name: "rmdir",
        main: applets::rmdir::main,
    },
    AppletEntry {
        name: "sort",
        main: applets::sort::main,
    },
    AppletEntry {
        name: "sleep",
        main: applets::sleep::main,
    },
    AppletEntry {
        name: "tar",
        main: applets::tar::main,
    },
    AppletEntry {
        name: "tee",
        main: applets::tee::main,
    },
    AppletEntry {
        name: "uname",
        main: applets::uname::main,
    },
    AppletEntry {
        name: "unlzma",
        main: applets::xz::main_unlzma,
    },
    AppletEntry {
        name: "unxz",
        main: applets::xz::main_unxz,
    },
    AppletEntry {
        name: "wc",
        main: applets::wc::main,
    },
    AppletEntry {
        name: "wget",
        main: applets::wget::main,
    },
    AppletEntry {
        name: "xz",
        main: applets::xz::main,
    },
    AppletEntry {
        name: "xzcat",
        main: applets::xz::main_xzcat,
    },
    AppletEntry {
        name: "zcat",
        main: applets::gzip::main_zcat,
    },
];

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

struct AppletInvocation<'a> {
    name: &'a str,
    args: &'a [String],
}

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
