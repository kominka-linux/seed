use std::collections::BTreeSet;
use std::ffi::OsString;

const APPLET_LIST_SOURCE: &str = include_str!("../applet_list.rs");

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut args = args.iter();

    let Some(command) = args.next().and_then(|value| value.to_str()) else {
        usage_and_exit();
    };

    match command {
        "list" => {
            if args.next().is_some() {
                usage_and_exit();
            }
            for applet in all_applets() {
                println!("{applet}");
            }
        }
        "man" => {
            let Some(applet) = args.next().and_then(|value| value.to_str()) else {
                usage_and_exit();
            };
            if args.next().is_some() {
                usage_and_exit();
            }
            if !all_applets().contains(&applet) {
                eprintln!("seed-docgen: unknown applet '{applet}'");
                std::process::exit(1);
            }

            let output = match applet {
                "wget" => seed::common::doc::render_manpage(
                    seed::wget_doc::doc(),
                    concat!("seed ", env!("CARGO_PKG_VERSION")),
                    "",
                ),
                "tar" => seed::common::doc::render_manpage(
                    seed::tar_doc::doc(),
                    concat!("seed ", env!("CARGO_PKG_VERSION")),
                    "",
                ),
                _ => seed::common::doc::render_generic_manpage(
                    applet,
                    concat!("seed ", env!("CARGO_PKG_VERSION")),
                    "",
                ),
            };

            print!("{output}");
        }
        _ => usage_and_exit(),
    }
}

fn usage_and_exit() -> ! {
    eprintln!("usage: seed-docgen <list|man> [applet]");
    std::process::exit(1);
}

fn all_applets() -> BTreeSet<&'static str> {
    APPLET_LIST_SOURCE
        .lines()
        .filter_map(|line| {
            let start = line.find("name: \"")? + 7;
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(&rest[..end])
        })
        .collect()
}
