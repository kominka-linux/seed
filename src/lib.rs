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
        name: "base32",
        main: applets::base64::main_base32,
    },
    AppletEntry {
        name: "base64",
        main: applets::base64::main_base64,
    },
    AppletEntry {
        name: "basename",
        main: applets::basename::main_basename,
    },
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
        name: "chgrp",
        main: applets::chgrp::main,
    },
    AppletEntry {
        name: "chmod",
        main: applets::chmod::main,
    },
    AppletEntry {
        name: "chown",
        main: applets::chown::main,
    },
    AppletEntry {
        name: "cmp",
        main: applets::cmp::main,
    },
    AppletEntry {
        name: "cp",
        main: applets::cp::main,
    },
    AppletEntry {
        name: "cut",
        main: applets::cut::main,
    },
    AppletEntry {
        name: "date",
        main: applets::date::main,
    },
    AppletEntry {
        name: "dd",
        main: applets::dd::main,
    },
    AppletEntry {
        name: "df",
        main: applets::df::main,
    },
    AppletEntry {
        name: "diff",
        main: applets::diff::main,
    },
    AppletEntry {
        name: "du",
        main: applets::du::main,
    },
    AppletEntry {
        name: "dirname",
        main: applets::basename::main_dirname,
    },
    AppletEntry {
        name: "echo",
        main: applets::echo::main,
    },
    AppletEntry {
        name: "egrep",
        main: applets::grep::main_extended,
    },
    AppletEntry {
        name: "env",
        main: applets::env::main,
    },
    AppletEntry {
        name: "expr",
        main: applets::expr::main,
    },
    AppletEntry {
        name: "flock",
        main: applets::flock::main,
    },
    AppletEntry {
        name: "fgrep",
        main: applets::grep::main_fixed,
    },
    AppletEntry {
        name: "find",
        main: applets::find::main,
    },
    AppletEntry {
        name: "fold",
        main: applets::fold::main,
    },
    AppletEntry {
        name: "false",
        main: applets::true_false::main_false,
    },
    AppletEntry {
        name: "fsync",
        main: applets::sync::main_fsync,
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
        name: "head",
        main: applets::head::main,
    },
    AppletEntry {
        name: "hexdump",
        main: applets::hexdump::main,
    },
    AppletEntry {
        name: "md5sum",
        main: applets::hashsum::main_md5sum,
    },
    AppletEntry {
        name: "sha1sum",
        main: applets::hashsum::main_sha1sum,
    },
    AppletEntry {
        name: "sha256sum",
        main: applets::hashsum::main_sha256sum,
    },
    AppletEntry {
        name: "sha512sum",
        main: applets::hashsum::main_sha512sum,
    },
    AppletEntry {
        name: "hostname",
        main: applets::hostname::main,
    },
    AppletEntry {
        name: "id",
        main: applets::id::main,
    },
    AppletEntry {
        name: "install",
        main: applets::install::main,
    },
    AppletEntry {
        name: "kill",
        main: applets::kill::main,
    },
    AppletEntry {
        name: "killall",
        main: applets::killall::main,
    },
    AppletEntry {
        name: "less",
        main: applets::less::main,
    },
    AppletEntry {
        name: "link",
        main: applets::link::main_link,
    },
    AppletEntry {
        name: "ln",
        main: applets::ln::main,
    },
    AppletEntry {
        name: "losetup",
        main: applets::losetup::main,
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
        name: "mkfifo",
        main: applets::mkfifo::main,
    },
    AppletEntry {
        name: "mktemp",
        main: applets::mktemp::main,
    },
    AppletEntry {
        name: "mknod",
        main: applets::mknod::main,
    },
    AppletEntry {
        name: "mv",
        main: applets::mv::main,
    },
    AppletEntry {
        name: "nohup",
        main: applets::nohup::main,
    },
    AppletEntry {
        name: "nproc",
        main: applets::nproc::main,
    },
    AppletEntry {
        name: "od",
        main: applets::od::main,
    },
    AppletEntry {
        name: "paste",
        main: applets::paste::main,
    },
    AppletEntry {
        name: "pgrep",
        main: applets::pgrep::main,
    },
    AppletEntry {
        name: "pkill",
        main: applets::pkill::main,
    },
    AppletEntry {
        name: "printenv",
        main: applets::printenv::main,
    },
    AppletEntry {
        name: "printf",
        main: applets::printf::main,
    },
    AppletEntry {
        name: "ps",
        main: applets::ps::main,
    },
    AppletEntry {
        name: "pwd",
        main: applets::pwd::main,
    },
    AppletEntry {
        name: "readlink",
        main: applets::readlink::main,
    },
    AppletEntry {
        name: "realpath",
        main: applets::realpath::main,
    },
    AppletEntry {
        name: "rev",
        main: applets::rev::main,
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
        name: "run-parts",
        main: applets::run_parts::main,
    },
    AppletEntry {
        name: "seq",
        main: applets::seq::main,
    },
    AppletEntry {
        name: "sleep",
        main: applets::sleep::main,
    },
    AppletEntry {
        name: "shuf",
        main: applets::shuf::main,
    },
    AppletEntry {
        name: "sort",
        main: applets::sort::main,
    },
    AppletEntry {
        name: "split",
        main: applets::split::main,
    },
    AppletEntry {
        name: "strings",
        main: applets::strings::main,
    },
    AppletEntry {
        name: "stat",
        main: applets::stat::main,
    },
    AppletEntry {
        name: "sync",
        main: applets::sync::main_sync,
    },
    AppletEntry {
        name: "tail",
        main: applets::tail::main,
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
        name: "test",
        main: applets::test::main_test,
    },
    AppletEntry {
        name: "time",
        main: applets::time::main,
    },
    AppletEntry {
        name: "[",
        main: applets::test::main_bracket,
    },
    AppletEntry {
        name: "[[",
        main: applets::test::main_double_bracket,
    },
    AppletEntry {
        name: "touch",
        main: applets::touch::main,
    },
    AppletEntry {
        name: "timeout",
        main: applets::timeout::main,
    },
    AppletEntry {
        name: "tr",
        main: applets::tr::main,
    },
    AppletEntry {
        name: "tree",
        main: applets::tree::main,
    },
    AppletEntry {
        name: "true",
        main: applets::true_false::main_true,
    },
    AppletEntry {
        name: "truncate",
        main: applets::truncate::main,
    },
    AppletEntry {
        name: "tty",
        main: applets::tty::main,
    },
    AppletEntry {
        name: "uname",
        main: applets::uname::main,
    },
    AppletEntry {
        name: "uniq",
        main: applets::uniq::main,
    },
    AppletEntry {
        name: "unlink",
        main: applets::link::main_unlink,
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
        name: "uptime",
        main: applets::uptime::main,
    },
    AppletEntry {
        name: "watch",
        main: applets::watch::main,
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
        name: "which",
        main: applets::which::main,
    },
    AppletEntry {
        name: "whoami",
        main: applets::whoami::main,
    },
    AppletEntry {
        name: "xargs",
        main: applets::xargs::main,
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
        name: "yes",
        main: applets::yes::main,
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
