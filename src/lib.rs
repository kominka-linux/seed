pub mod applets;
pub mod common;

use std::path::Path;

pub fn dispatch(argv: &[String]) -> i32 {
    let Some(applet) = determine_applet(argv) else {
        eprintln!("seed: missing applet name");
        return 1;
    };

    match applet.name.as_str() {
        "bunzip2" => applets::bzip2::main_bunzip2(applet.args),
        "bzip2" => applets::bzip2::main(applet.args),
        "bzcat" => applets::bzip2::main_bzcat(applet.args),
        "cat" => applets::cat::main(applet.args),
        "chmod" => applets::chmod::main(applet.args),
        "cp" => applets::cp::main(applet.args),
        "diff" => applets::diff::main(applet.args),
        "egrep" => applets::grep::main_extended(applet.args),
        "find" => applets::find::main(applet.args),
        "grep" => applets::grep::main(applet.args),
        "gunzip" => applets::gzip::main_gunzip(applet.args),
        "gzip" => applets::gzip::main(applet.args),
        "mkdir" => applets::mkdir::main(applet.args),
        "ls" => applets::ls::main(applet.args),
        "mv" => applets::mv::main(applet.args),
        "od" => applets::od::main(applet.args),
        "rm" => applets::rm::main(applet.args),
        "rmdir" => applets::rmdir::main(applet.args),
        "printf" => applets::printf::main(applet.args),
        "sort" => applets::sort::main(applet.args),
        "tar" => applets::tar::main(applet.args),
        "tee" => applets::tee::main(applet.args),
        "lzcat" => applets::xz::main_lzcat(applet.args),
        "lzma" => applets::xz::main_lzma(applet.args),
        "unlzma" => applets::xz::main_unlzma(applet.args),
        "unxz" => applets::xz::main_unxz(applet.args),
        "wc" => applets::wc::main(applet.args),
        "xz" => applets::xz::main(applet.args),
        "xzcat" => applets::xz::main_xzcat(applet.args),
        "zcat" => applets::gzip::main_zcat(applet.args),
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
