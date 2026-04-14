macro_rules! define_applet_modules {
    () => {
        pub mod addgroup;
        pub mod adduser;
        pub mod base64;
        pub mod basename;
        #[cfg(target_os = "linux")]
        pub mod blkid;
        pub mod bzip2;
        pub mod cat;
        pub mod chattr;
        pub mod chgrp;
        pub mod chmod;
        pub mod chown;
        #[cfg(target_os = "linux")]
        pub mod chroot;
        pub mod cmp;
        pub mod cp;
        pub mod cpio;
        pub mod cut;
        pub mod date;
        pub mod dd;
        pub mod delgroup;
        pub mod deluser;
        #[cfg(target_os = "linux")]
        pub mod df;
        pub mod diff;
        #[cfg(target_os = "linux")]
        pub mod dmesg;
        pub mod du;
        pub mod echo;
        pub mod env;
        pub mod expr;
        #[cfg(target_os = "linux")]
        pub mod fdisk;
        pub mod find;
        pub mod flock;
        pub mod fold;
        pub mod free;
        pub mod fsck;
        pub mod getopt;
        pub mod getty;
        pub mod grep;
        pub mod gzip;
        pub mod hashsum;
        pub mod head;
        pub mod hexdump;
        pub mod hostname;
        #[cfg(target_os = "linux")]
        pub mod hwclock;
        pub mod id;
        #[cfg(target_os = "linux")]
        pub mod ifconfig;
        #[cfg(target_os = "linux")]
        pub mod insmod;
        pub mod install;
        #[cfg(target_os = "linux")]
        pub mod ip;
        pub mod kill;
        pub mod killall;
        #[cfg(target_os = "linux")]
        pub mod killall5;
        pub mod less;
        pub mod link;
        pub mod ln;
        pub mod login;
        #[cfg(target_os = "linux")]
        pub mod losetup;
        pub mod ls;
        pub mod lsattr;
        #[cfg(target_os = "linux")]
        pub mod lsmod;
        #[cfg(target_os = "linux")]
        pub mod lsof;
        pub mod man;
        pub mod mkdir;
        pub mod mkfifo;
        #[cfg(target_os = "linux")]
        pub mod mknod;
        pub mod mkswap;
        pub mod mktemp;
        #[cfg(target_os = "linux")]
        pub mod mount;
        #[cfg(target_os = "linux")]
        pub mod mountpoint;
        pub mod mv;
        #[cfg(target_os = "linux")]
        pub mod netstat;
        pub mod nohup;
        pub mod nologin;
        pub mod nproc;
        pub mod nslookup;
        pub mod ntpd;
        pub mod od;
        pub mod passwd;
        pub mod paste;
        pub mod patch;
        pub mod pgrep;
        pub mod pidof;
        #[cfg(target_os = "linux")]
        pub mod ping;
        #[cfg(target_os = "linux")]
        pub mod pivot_root;
        pub mod pkill;
        pub mod printenv;
        pub mod printf;
        pub mod ps;
        pub mod pstree;
        pub mod pwd;
        pub mod readlink;
        pub mod realpath;
        pub mod rev;
        #[cfg(target_os = "linux")]
        pub mod rfkill;
        pub mod rm;
        pub mod rmdir;
        #[cfg(target_os = "linux")]
        pub mod rmmod;
        pub mod run_parts;
        pub mod seq;
        pub mod setsid;
        pub mod shuf;
        #[cfg(target_os = "linux")]
        pub mod shutdown;
        pub mod sleep;
        pub mod sort;
        pub mod split;
        pub mod stat;
        pub mod strings;
        pub mod stty;
        pub mod su;
        pub mod sulogin;
        #[cfg(target_os = "linux")]
        pub mod swapoff;
        #[cfg(target_os = "linux")]
        pub mod swapon;
        #[cfg(target_os = "linux")]
        pub mod switch_root;
        pub mod sync;
        #[cfg(target_os = "linux")]
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
        pub mod udhcpc;
        pub mod udhcpd;
        #[cfg(target_os = "linux")]
        pub mod umount;
        pub mod uname;
        pub mod uniq;
        pub mod unzip;
        #[cfg(target_os = "linux")]
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
                name: "addgroup",
                main: applets::addgroup::main,
            },
            $entry {
                name: "adduser",
                main: applets::adduser::main,
            },
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "blkid",
                main: applets::blkid::main,
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
                name: "chattr",
                main: applets::chattr::main,
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
            #[cfg(target_os = "linux")]
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
                name: "cpio",
                main: applets::cpio::main,
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
                name: "delgroup",
                main: applets::delgroup::main,
            },
            $entry {
                name: "deluser",
                main: applets::deluser::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "df",
                main: applets::df::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "dmesg",
                main: applets::dmesg::main,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "fdisk",
                main: applets::fdisk::main,
            },
            $entry {
                name: "fsck",
                main: applets::fsck::main,
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
                name: "getty",
                main: applets::getty::main,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "halt",
                main: applets::shutdown::main_halt,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "hwclock",
                main: applets::hwclock::main,
            },
            $entry {
                name: "id",
                main: applets::id::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "ifconfig",
                main: applets::ifconfig::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "ip",
                main: applets::ip::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "insmod",
                main: applets::insmod::main,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "killall5",
                main: applets::killall5::main,
            },
            $entry {
                name: "less",
                main: applets::less::main,
            },
            $entry {
                name: "login",
                main: applets::login::main,
            },
            $entry {
                name: "link",
                main: applets::link::main_link,
            },
            $entry {
                name: "ln",
                main: applets::ln::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "losetup",
                main: applets::losetup::main,
            },
            $entry {
                name: "ls",
                main: applets::ls::main,
            },
            $entry {
                name: "lsattr",
                main: applets::lsattr::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "lsmod",
                main: applets::lsmod::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "lsof",
                main: applets::lsof::main,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "mknod",
                main: applets::mknod::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "mountpoint",
                main: applets::mountpoint::main,
            },
            $entry {
                name: "mktemp",
                main: applets::mktemp::main,
            },
            $entry {
                name: "mkswap",
                main: applets::mkswap::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "mount",
                main: applets::mount::main,
            },
            $entry {
                name: "mv",
                main: applets::mv::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "netstat",
                main: applets::netstat::main,
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
                name: "ntpd",
                main: applets::ntpd::main,
            },
            $entry {
                name: "udhcpc",
                main: applets::udhcpc::main,
            },
            $entry {
                name: "udhcpd",
                main: applets::udhcpd::main,
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
                name: "passwd",
                main: applets::passwd::main,
            },
            $entry {
                name: "paste",
                main: applets::paste::main,
            },
            $entry {
                name: "patch",
                main: applets::patch::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "ping",
                main: applets::ping::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "ping6",
                main: applets::ping::main_ping6,
            },
            $entry {
                name: "pgrep",
                main: applets::pgrep::main,
            },
            $entry {
                name: "pidof",
                main: applets::pidof::main,
            },
            $entry {
                name: "pkill",
                main: applets::pkill::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "pivot_root",
                main: applets::pivot_root::main,
            },
            $entry {
                name: "printenv",
                main: applets::printenv::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "poweroff",
                main: applets::shutdown::main_poweroff,
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
                name: "pstree",
                main: applets::pstree::main,
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "rfkill",
                main: applets::rfkill::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "reboot",
                main: applets::shutdown::main_reboot,
            },
            $entry {
                name: "rev",
                main: applets::rev::main,
            },
            $entry {
                name: "rm",
                main: applets::rm::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "rmmod",
                main: applets::rmmod::main,
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
                name: "stty",
                main: applets::stty::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "swapoff",
                main: applets::swapoff::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "swapon",
                main: applets::swapon::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "switch_root",
                main: applets::switch_root::main,
            },
            $entry {
                name: "su",
                main: applets::su::main,
            },
            $entry {
                name: "sulogin",
                main: applets::sulogin::main,
            },
            $entry {
                name: "sync",
                main: applets::sync::main_sync,
            },
            #[cfg(target_os = "linux")]
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
            #[cfg(target_os = "linux")]
            $entry {
                name: "umount",
                main: applets::umount::main,
            },
            #[cfg(target_os = "linux")]
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
