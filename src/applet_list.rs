macro_rules! define_applet_modules {
    () => {
        pub mod base64;
        pub mod basename;
        pub mod bzip2;
        pub mod cat;
        pub mod chgrp;
        pub mod chmod;
        pub mod chown;
        pub mod chroot;
        pub mod cmp;
        pub mod cp;
        pub mod cut;
        pub mod date;
        pub mod dd;
        pub mod df;
        pub mod diff;
        pub mod du;
        pub mod echo;
        pub mod env;
        pub mod expr;
        pub mod find;
        pub mod flock;
        pub mod fold;
        pub mod free;
        pub mod getopt;
        pub mod grep;
        pub mod gzip;
        pub mod hashsum;
        pub mod head;
        pub mod hexdump;
        pub mod hostname;
        pub mod id;
        pub mod install;
        pub mod kill;
        pub mod killall;
        pub mod less;
        pub mod link;
        pub mod ln;
        pub mod losetup;
        pub mod ls;
        pub mod man;
        pub mod mkdir;
        pub mod mkfifo;
        pub mod mknod;
        pub mod mktemp;
        pub mod mountpoint;
        pub mod mv;
        pub mod nohup;
        pub mod nologin;
        pub mod nproc;
        pub mod nslookup;
        pub mod od;
        pub mod paste;
        pub mod patch;
        pub mod pgrep;
        pub mod pkill;
        pub mod printenv;
        pub mod printf;
        pub mod ps;
        pub mod pwd;
        pub mod readlink;
        pub mod realpath;
        pub mod rev;
        pub mod rm;
        pub mod rmdir;
        pub mod run_parts;
        pub mod seq;
        pub mod setsid;
        pub mod shuf;
        pub mod sleep;
        pub mod sort;
        pub mod split;
        pub mod stat;
        pub mod strings;
        pub mod sync;
        pub mod sysctl;
        pub mod tail;
        pub mod tar;
        pub mod tee;
        pub mod test;
        pub mod time;
        pub mod timeout;
        pub mod touch;
        pub mod tr;
        pub mod tree;
        pub mod true_false;
        pub mod truncate;
        pub mod tty;
        pub mod uname;
        pub mod uniq;
        pub mod unzip;
        pub mod uptime;
        pub mod watch;
        pub mod wc;
        pub mod wget;
        pub mod which;
        pub mod whoami;
        pub mod xargs;
        pub mod xz;
        pub mod yes;
    };
}

macro_rules! define_applet_entries {
    ($entry:ident) => {
        &[
            $entry {
                name: "base32",
                main: applets::base64::main_base32,
            },
            $entry {
                name: "base64",
                main: applets::base64::main_base64,
            },
            $entry {
                name: "basename",
                main: applets::basename::main_basename,
            },
            $entry {
                name: "bunzip2",
                main: applets::bzip2::main_bunzip2,
            },
            $entry {
                name: "bzip2",
                main: applets::bzip2::main,
            },
            $entry {
                name: "bzcat",
                main: applets::bzip2::main_bzcat,
            },
            $entry {
                name: "cat",
                main: applets::cat::main,
            },
            $entry {
                name: "chgrp",
                main: applets::chgrp::main,
            },
            $entry {
                name: "chmod",
                main: applets::chmod::main,
            },
            $entry {
                name: "chown",
                main: applets::chown::main,
            },
            $entry {
                name: "chroot",
                main: applets::chroot::main,
            },
            $entry {
                name: "cmp",
                main: applets::cmp::main,
            },
            $entry {
                name: "cp",
                main: applets::cp::main,
            },
            $entry {
                name: "cut",
                main: applets::cut::main,
            },
            $entry {
                name: "date",
                main: applets::date::main,
            },
            $entry {
                name: "dd",
                main: applets::dd::main,
            },
            $entry {
                name: "df",
                main: applets::df::main,
            },
            $entry {
                name: "diff",
                main: applets::diff::main,
            },
            $entry {
                name: "du",
                main: applets::du::main,
            },
            $entry {
                name: "dirname",
                main: applets::basename::main_dirname,
            },
            $entry {
                name: "echo",
                main: applets::echo::main,
            },
            $entry {
                name: "egrep",
                main: applets::grep::main_extended,
            },
            $entry {
                name: "env",
                main: applets::env::main,
            },
            $entry {
                name: "expr",
                main: applets::expr::main,
            },
            $entry {
                name: "flock",
                main: applets::flock::main,
            },
            $entry {
                name: "fgrep",
                main: applets::grep::main_fixed,
            },
            $entry {
                name: "find",
                main: applets::find::main,
            },
            $entry {
                name: "fold",
                main: applets::fold::main,
            },
            $entry {
                name: "false",
                main: applets::true_false::main_false,
            },
            $entry {
                name: "fsync",
                main: applets::sync::main_fsync,
            },
            $entry {
                name: "free",
                main: applets::free::main,
            },
            $entry {
                name: "getopt",
                main: applets::getopt::main,
            },
            $entry {
                name: "grep",
                main: applets::grep::main,
            },
            $entry {
                name: "gunzip",
                main: applets::gzip::main_gunzip,
            },
            $entry {
                name: "gzip",
                main: applets::gzip::main,
            },
            $entry {
                name: "head",
                main: applets::head::main,
            },
            $entry {
                name: "hexdump",
                main: applets::hexdump::main,
            },
            $entry {
                name: "md5sum",
                main: applets::hashsum::main_md5sum,
            },
            $entry {
                name: "sha1sum",
                main: applets::hashsum::main_sha1sum,
            },
            $entry {
                name: "sha256sum",
                main: applets::hashsum::main_sha256sum,
            },
            $entry {
                name: "sha512sum",
                main: applets::hashsum::main_sha512sum,
            },
            $entry {
                name: "hostname",
                main: applets::hostname::main,
            },
            $entry {
                name: "id",
                main: applets::id::main,
            },
            $entry {
                name: "install",
                main: applets::install::main,
            },
            $entry {
                name: "kill",
                main: applets::kill::main,
            },
            $entry {
                name: "killall",
                main: applets::killall::main,
            },
            $entry {
                name: "less",
                main: applets::less::main,
            },
            $entry {
                name: "link",
                main: applets::link::main_link,
            },
            $entry {
                name: "ln",
                main: applets::ln::main,
            },
            $entry {
                name: "losetup",
                main: applets::losetup::main,
            },
            $entry {
                name: "ls",
                main: applets::ls::main,
            },
            $entry {
                name: "lzcat",
                main: applets::xz::main_lzcat,
            },
            $entry {
                name: "lzma",
                main: applets::xz::main_lzma,
            },
            $entry {
                name: "man",
                main: applets::man::main,
            },
            $entry {
                name: "mkdir",
                main: applets::mkdir::main,
            },
            $entry {
                name: "mkfifo",
                main: applets::mkfifo::main,
            },
            $entry {
                name: "mknod",
                main: applets::mknod::main,
            },
            $entry {
                name: "mountpoint",
                main: applets::mountpoint::main,
            },
            $entry {
                name: "mktemp",
                main: applets::mktemp::main,
            },
            $entry {
                name: "mv",
                main: applets::mv::main,
            },
            $entry {
                name: "nohup",
                main: applets::nohup::main,
            },
            $entry {
                name: "nologin",
                main: applets::nologin::main,
            },
            $entry {
                name: "nproc",
                main: applets::nproc::main,
            },
            $entry {
                name: "nslookup",
                main: applets::nslookup::main,
            },
            $entry {
                name: "od",
                main: applets::od::main,
            },
            $entry {
                name: "paste",
                main: applets::paste::main,
            },
            $entry {
                name: "patch",
                main: applets::patch::main,
            },
            $entry {
                name: "pgrep",
                main: applets::pgrep::main,
            },
            $entry {
                name: "pkill",
                main: applets::pkill::main,
            },
            $entry {
                name: "printenv",
                main: applets::printenv::main,
            },
            $entry {
                name: "printf",
                main: applets::printf::main,
            },
            $entry {
                name: "ps",
                main: applets::ps::main,
            },
            $entry {
                name: "pwd",
                main: applets::pwd::main,
            },
            $entry {
                name: "readlink",
                main: applets::readlink::main,
            },
            $entry {
                name: "realpath",
                main: applets::realpath::main,
            },
            $entry {
                name: "rev",
                main: applets::rev::main,
            },
            $entry {
                name: "rm",
                main: applets::rm::main,
            },
            $entry {
                name: "rmdir",
                main: applets::rmdir::main,
            },
            $entry {
                name: "run-parts",
                main: applets::run_parts::main,
            },
            $entry {
                name: "seq",
                main: applets::seq::main,
            },
            $entry {
                name: "setsid",
                main: applets::setsid::main,
            },
            $entry {
                name: "shuf",
                main: applets::shuf::main,
            },
            $entry {
                name: "sleep",
                main: applets::sleep::main,
            },
            $entry {
                name: "sort",
                main: applets::sort::main,
            },
            $entry {
                name: "split",
                main: applets::split::main,
            },
            $entry {
                name: "stat",
                main: applets::stat::main,
            },
            $entry {
                name: "strings",
                main: applets::strings::main,
            },
            $entry {
                name: "sync",
                main: applets::sync::main_sync,
            },
            $entry {
                name: "sysctl",
                main: applets::sysctl::main,
            },
            $entry {
                name: "tail",
                main: applets::tail::main,
            },
            $entry {
                name: "tar",
                main: applets::tar::main,
            },
            $entry {
                name: "tee",
                main: applets::tee::main,
            },
            $entry {
                name: "test",
                main: applets::test::main_test,
            },
            $entry {
                name: "time",
                main: applets::time::main,
            },
            $entry {
                name: "[",
                main: applets::test::main_bracket,
            },
            $entry {
                name: "[[",
                main: applets::test::main_double_bracket,
            },
            $entry {
                name: "touch",
                main: applets::touch::main,
            },
            $entry {
                name: "timeout",
                main: applets::timeout::main,
            },
            $entry {
                name: "tr",
                main: applets::tr::main,
            },
            $entry {
                name: "tree",
                main: applets::tree::main,
            },
            $entry {
                name: "true",
                main: applets::true_false::main_true,
            },
            $entry {
                name: "truncate",
                main: applets::truncate::main,
            },
            $entry {
                name: "tty",
                main: applets::tty::main,
            },
            $entry {
                name: "uname",
                main: applets::uname::main,
            },
            $entry {
                name: "uniq",
                main: applets::uniq::main,
            },
            $entry {
                name: "unlink",
                main: applets::link::main_unlink,
            },
            $entry {
                name: "unlzma",
                main: applets::xz::main_unlzma,
            },
            $entry {
                name: "unxz",
                main: applets::xz::main_unxz,
            },
            $entry {
                name: "unzip",
                main: applets::unzip::main,
            },
            $entry {
                name: "uptime",
                main: applets::uptime::main,
            },
            $entry {
                name: "watch",
                main: applets::watch::main,
            },
            $entry {
                name: "wc",
                main: applets::wc::main,
            },
            $entry {
                name: "wget",
                main: applets::wget::main,
            },
            $entry {
                name: "which",
                main: applets::which::main,
            },
            $entry {
                name: "whoami",
                main: applets::whoami::main,
            },
            $entry {
                name: "xargs",
                main: applets::xargs::main,
            },
            $entry {
                name: "xz",
                main: applets::xz::main,
            },
            $entry {
                name: "xzcat",
                main: applets::xz::main_xzcat,
            },
            $entry {
                name: "yes",
                main: applets::yes::main,
            },
            $entry {
                name: "zcat",
                main: applets::gzip::main_zcat,
            },
        ]
    };
}
