macro_rules! define_applet_modules {
    () => {
        #[cfg(target_os = "linux")]
        pub mod acpid;
        pub mod addgroup;
        pub mod adduser;
        #[cfg(feature = "deprecated-applets")]
        pub mod awk;
        #[cfg(feature = "deprecated-applets")]
        pub mod base64;
        #[cfg(feature = "deprecated-applets")]
        pub mod basename;
        #[cfg(target_os = "linux")]
        pub mod blkid;
        #[cfg(feature = "deprecated-applets")]
        pub mod bzip2;
        #[cfg(feature = "deprecated-applets")]
        pub mod cat;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod chattr;
        #[cfg(feature = "deprecated-applets")]
        pub mod chgrp;
        #[cfg(feature = "deprecated-applets")]
        pub mod chmod;
        #[cfg(feature = "deprecated-applets")]
        pub mod chown;
        #[cfg(target_os = "linux")]
        pub mod chroot;
        #[cfg(feature = "deprecated-applets")]
        pub mod cmp;
        #[cfg(feature = "deprecated-applets")]
        pub mod cp;
        #[cfg(feature = "deprecated-applets")]
        pub mod cpio;
        #[cfg(feature = "deprecated-applets")]
        pub mod cut;
        #[cfg(feature = "deprecated-applets")]
        pub mod date;
        #[cfg(feature = "deprecated-applets")]
        pub mod dd;
        pub mod delgroup;
        pub mod deluser;
        #[cfg(target_os = "linux")]
        pub mod depmod;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod df;
        #[cfg(feature = "deprecated-applets")]
        pub mod diff;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod dmesg;
        #[cfg(feature = "deprecated-applets")]
        pub mod du;
        #[cfg(feature = "deprecated-applets")]
        pub mod echo;
        #[cfg(feature = "deprecated-applets")]
        pub mod env;
        #[cfg(feature = "deprecated-applets")]
        pub mod expr;
        #[cfg(target_os = "linux")]
        pub mod fdisk;
        #[cfg(feature = "deprecated-applets")]
        pub mod find;
        #[cfg(feature = "deprecated-applets")]
        pub mod flock;
        #[cfg(feature = "deprecated-applets")]
        pub mod fold;
        #[cfg(feature = "deprecated-applets")]
        pub mod free;
        pub mod fsck;
        #[cfg(feature = "deprecated-applets")]
        pub mod getopt;
        pub mod getty;
        #[cfg(feature = "deprecated-applets")]
        pub mod grep;
        #[cfg(feature = "deprecated-applets")]
        pub mod gzip;
        #[cfg(feature = "deprecated-applets")]
        pub mod hashsum;
        #[cfg(feature = "deprecated-applets")]
        pub mod head;
        #[cfg(feature = "deprecated-applets")]
        pub mod hexdump;
        #[cfg(feature = "deprecated-applets")]
        pub mod hostname;
        #[cfg(target_os = "linux")]
        pub mod hwclock;
        #[cfg(feature = "deprecated-applets")]
        pub mod id;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod ifconfig;
        #[cfg(target_os = "linux")]
        pub mod init;
        #[cfg(target_os = "linux")]
        pub mod insmod;
        #[cfg(feature = "deprecated-applets")]
        pub mod install;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod ip;
        #[cfg(feature = "deprecated-applets")]
        pub mod kill;
        #[cfg(feature = "deprecated-applets")]
        pub mod killall;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod killall5;
        pub mod less;
        #[cfg(feature = "deprecated-applets")]
        pub mod link;
        #[cfg(feature = "deprecated-applets")]
        pub mod ln;
        pub mod login;
        #[cfg(target_os = "linux")]
        pub mod losetup;
        #[cfg(feature = "deprecated-applets")]
        pub mod ls;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod lsattr;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod lsmod;
        #[cfg(target_os = "linux")]
        pub mod lsof;
        pub mod man;
        #[cfg(target_os = "linux")]
        pub mod mdev;
        #[cfg(feature = "deprecated-applets")]
        pub mod mkdir;
        #[cfg(feature = "deprecated-applets")]
        pub mod mkfifo;
        #[cfg(target_os = "linux")]
        pub mod mknod;
        pub mod mkswap;
        #[cfg(feature = "deprecated-applets")]
        pub mod mktemp;
        #[cfg(target_os = "linux")]
        pub mod modinfo;
        #[cfg(target_os = "linux")]
        pub mod modprobe;
        #[cfg(target_os = "linux")]
        pub mod mount;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod mountpoint;
        #[cfg(feature = "deprecated-applets")]
        pub mod mv;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod netstat;
        #[cfg(feature = "deprecated-applets")]
        pub mod nohup;
        pub mod nologin;
        #[cfg(feature = "deprecated-applets")]
        pub mod nproc;
        #[cfg(feature = "deprecated-applets")]
        pub mod nslookup;
        #[cfg(feature = "deprecated-applets")]
        pub mod ntpd;
        #[cfg(feature = "deprecated-applets")]
        pub mod od;
        pub mod passwd;
        #[cfg(feature = "deprecated-applets")]
        pub mod paste;
        #[cfg(feature = "deprecated-applets")]
        pub mod patch;
        #[cfg(feature = "deprecated-applets")]
        pub mod pgrep;
        #[cfg(feature = "deprecated-applets")]
        pub mod pidof;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod ping;
        #[cfg(target_os = "linux")]
        pub mod pivot_root;
        #[cfg(feature = "deprecated-applets")]
        pub mod pkill;
        #[cfg(feature = "deprecated-applets")]
        pub mod printenv;
        #[cfg(feature = "deprecated-applets")]
        pub mod printf;
        #[cfg(feature = "deprecated-applets")]
        pub mod ps;
        #[cfg(feature = "deprecated-applets")]
        pub mod pstree;
        #[cfg(feature = "deprecated-applets")]
        pub mod pwd;
        #[cfg(feature = "deprecated-applets")]
        pub mod readlink;
        #[cfg(feature = "deprecated-applets")]
        pub mod realpath;
        #[cfg(feature = "deprecated-applets")]
        pub mod rev;
        #[cfg(target_os = "linux")]
        pub mod rfkill;
        #[cfg(feature = "deprecated-applets")]
        pub mod rm;
        #[cfg(feature = "deprecated-applets")]
        pub mod rmdir;
        #[cfg(target_os = "linux")]
        pub mod rmmod;
        #[cfg(feature = "deprecated-applets")]
        pub mod run_parts;
        #[cfg(feature = "deprecated-applets")]
        pub mod sed;
        #[cfg(feature = "deprecated-applets")]
        pub mod seq;
        #[cfg(feature = "deprecated-applets")]
        pub mod setsid;
        #[cfg(feature = "deprecated-applets")]
        pub mod shuf;
        #[cfg(target_os = "linux")]
        pub mod shutdown;
        #[cfg(feature = "deprecated-applets")]
        pub mod sleep;
        #[cfg(feature = "deprecated-applets")]
        pub mod sort;
        #[cfg(feature = "deprecated-applets")]
        pub mod split;
        #[cfg(feature = "deprecated-applets")]
        pub mod stat;
        #[cfg(feature = "deprecated-applets")]
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
        #[cfg(feature = "deprecated-applets")]
        pub mod sync;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod sysctl;
        #[cfg(feature = "deprecated-applets")]
        pub mod tail;
        #[cfg(feature = "deprecated-applets")]
        pub mod tar;
        #[cfg(feature = "deprecated-applets")]
        pub mod tee;
        #[cfg(feature = "deprecated-applets")]
        pub mod test;
        #[cfg(feature = "deprecated-applets")]
        pub mod time;
        #[cfg(feature = "deprecated-applets")]
        pub mod timeout;
        #[cfg(feature = "deprecated-applets")]
        pub mod touch;
        #[cfg(feature = "deprecated-applets")]
        pub mod tr;
        #[cfg(feature = "deprecated-applets")]
        pub mod tree;
        #[cfg(feature = "deprecated-applets")]
        pub mod true_false;
        #[cfg(feature = "deprecated-applets")]
        pub mod truncate;
        #[cfg(feature = "deprecated-applets")]
        pub mod tty;
        #[cfg(feature = "deprecated-applets")]
        pub mod udhcpc;
        #[cfg(feature = "deprecated-applets")]
        pub mod udhcpd;
        #[cfg(target_os = "linux")]
        pub mod umount;
        #[cfg(feature = "deprecated-applets")]
        pub mod uname;
        #[cfg(feature = "deprecated-applets")]
        pub mod uniq;
        #[cfg(feature = "deprecated-applets")]
        pub mod unzip;
        #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
        pub mod uptime;
        #[cfg(feature = "deprecated-applets")]
        pub mod watch;
        #[cfg(feature = "deprecated-applets")]
        pub mod wc;
        #[cfg(feature = "deprecated-applets")]
        pub mod wget;
        #[cfg(feature = "deprecated-applets")]
        pub mod which;
        #[cfg(feature = "deprecated-applets")]
        pub mod whoami;
        #[cfg(feature = "deprecated-applets")]
        pub mod xargs;
        #[cfg(feature = "deprecated-applets")]
        pub mod xz;
        #[cfg(feature = "deprecated-applets")]
        pub mod yes;
    };
}

macro_rules! define_applet_entries {
    ($entry:ident) => {
        &[
            #[cfg(target_os = "linux")]
            $entry {
                name: "acpid",
                main: applets::acpid::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "awk",
                main: applets::awk::main,
            },
            $entry {
                name: "addgroup",
                main: applets::addgroup::main,
            },
            $entry {
                name: "adduser",
                main: applets::adduser::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "base32",
                main: applets::base64::main_base32,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "base64",
                main: applets::base64::main_base64,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "basename",
                main: applets::basename::main_basename,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "blkid",
                main: applets::blkid::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "bunzip2",
                main: applets::bzip2::main_bunzip2,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "bzip2",
                main: applets::bzip2::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "bzcat",
                main: applets::bzip2::main_bzcat,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "cat",
                main: applets::cat::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "chattr",
                main: applets::chattr::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "chgrp",
                main: applets::chgrp::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "chmod",
                main: applets::chmod::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "chown",
                main: applets::chown::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "chroot",
                main: applets::chroot::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "cmp",
                main: applets::cmp::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "cp",
                main: applets::cp::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "cpio",
                main: applets::cpio::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "cut",
                main: applets::cut::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "date",
                main: applets::date::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "dd",
                main: applets::dd::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "depmod",
                main: applets::depmod::main,
            },
            $entry {
                name: "delgroup",
                main: applets::delgroup::main,
            },
            $entry {
                name: "deluser",
                main: applets::deluser::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "df",
                main: applets::df::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "dmesg",
                main: applets::dmesg::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "diff",
                main: applets::diff::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "du",
                main: applets::du::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "dirname",
                main: applets::basename::main_dirname,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "echo",
                main: applets::echo::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "egrep",
                main: applets::grep::main_extended,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "env",
                main: applets::env::main,
            },
            #[cfg(feature = "deprecated-applets")]
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
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "flock",
                main: applets::flock::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "fgrep",
                main: applets::grep::main_fixed,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "find",
                main: applets::find::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "fold",
                main: applets::fold::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "false",
                main: applets::true_false::main_false,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "fsync",
                main: applets::sync::main_fsync,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "free",
                main: applets::free::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "getopt",
                main: applets::getopt::main,
            },
            $entry {
                name: "getty",
                main: applets::getty::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "grep",
                main: applets::grep::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "gunzip",
                main: applets::gzip::main_gunzip,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "gzip",
                main: applets::gzip::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "halt",
                main: applets::shutdown::main_halt,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "head",
                main: applets::head::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "hexdump",
                main: applets::hexdump::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "md5sum",
                main: applets::hashsum::main_md5sum,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sha1sum",
                main: applets::hashsum::main_sha1sum,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sha256sum",
                main: applets::hashsum::main_sha256sum,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sha512sum",
                main: applets::hashsum::main_sha512sum,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "hostname",
                main: applets::hostname::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "hwclock",
                main: applets::hwclock::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "id",
                main: applets::id::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "init",
                main: applets::init::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "ifconfig",
                main: applets::ifconfig::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "ip",
                main: applets::ip::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "insmod",
                main: applets::insmod::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "install",
                main: applets::install::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "kill",
                main: applets::kill::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "killall",
                main: applets::killall::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
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
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "link",
                main: applets::link::main_link,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "ln",
                main: applets::ln::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "losetup",
                main: applets::losetup::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "ls",
                main: applets::ls::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "lsattr",
                main: applets::lsattr::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "lsmod",
                main: applets::lsmod::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "lsof",
                main: applets::lsof::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "lzcat",
                main: applets::xz::main_lzcat,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "lzma",
                main: applets::xz::main_lzma,
            },
            $entry {
                name: "man",
                main: applets::man::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "mkdir",
                main: applets::mkdir::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "mkfifo",
                main: applets::mkfifo::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "mdev",
                main: applets::mdev::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "mknod",
                main: applets::mknod::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "mountpoint",
                main: applets::mountpoint::main,
            },
            #[cfg(feature = "deprecated-applets")]
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
                name: "modinfo",
                main: applets::modinfo::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "modprobe",
                main: applets::modprobe::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "mount",
                main: applets::mount::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "mv",
                main: applets::mv::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "netstat",
                main: applets::netstat::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "nohup",
                main: applets::nohup::main,
            },
            $entry {
                name: "nologin",
                main: applets::nologin::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "nproc",
                main: applets::nproc::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "ntpd",
                main: applets::ntpd::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "udhcpc",
                main: applets::udhcpc::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "udhcpd",
                main: applets::udhcpd::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "nslookup",
                main: applets::nslookup::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "od",
                main: applets::od::main,
            },
            $entry {
                name: "passwd",
                main: applets::passwd::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "paste",
                main: applets::paste::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "patch",
                main: applets::patch::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "ping",
                main: applets::ping::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "ping6",
                main: applets::ping::main_ping6,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "pgrep",
                main: applets::pgrep::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "pidof",
                main: applets::pidof::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "pkill",
                main: applets::pkill::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "pivot_root",
                main: applets::pivot_root::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "printenv",
                main: applets::printenv::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "poweroff",
                main: applets::shutdown::main_poweroff,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "printf",
                main: applets::printf::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "ps",
                main: applets::ps::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "pstree",
                main: applets::pstree::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "pwd",
                main: applets::pwd::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "readlink",
                main: applets::readlink::main,
            },
            #[cfg(feature = "deprecated-applets")]
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
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "rev",
                main: applets::rev::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "rm",
                main: applets::rm::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "rmmod",
                main: applets::rmmod::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "rmdir",
                main: applets::rmdir::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "run-parts",
                main: applets::run_parts::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "seq",
                main: applets::seq::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sed",
                main: applets::sed::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "setsid",
                main: applets::setsid::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "shuf",
                main: applets::shuf::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sleep",
                main: applets::sleep::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sort",
                main: applets::sort::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "split",
                main: applets::split::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "stat",
                main: applets::stat::main,
            },
            #[cfg(feature = "deprecated-applets")]
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
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "sync",
                main: applets::sync::main_sync,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "sysctl",
                main: applets::sysctl::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tail",
                main: applets::tail::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tar",
                main: applets::tar::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tee",
                main: applets::tee::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "test",
                main: applets::test::main_test,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "time",
                main: applets::time::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "[",
                main: applets::test::main_bracket,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "[[",
                main: applets::test::main_double_bracket,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "touch",
                main: applets::touch::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "timeout",
                main: applets::timeout::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tr",
                main: applets::tr::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tree",
                main: applets::tree::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "true",
                main: applets::true_false::main_true,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "truncate",
                main: applets::truncate::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "tty",
                main: applets::tty::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "uname",
                main: applets::uname::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "uniq",
                main: applets::uniq::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "unlink",
                main: applets::link::main_unlink,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "unlzma",
                main: applets::xz::main_unlzma,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "unxz",
                main: applets::xz::main_unxz,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "unzip",
                main: applets::unzip::main,
            },
            #[cfg(target_os = "linux")]
            $entry {
                name: "umount",
                main: applets::umount::main,
            },
            #[cfg(all(target_os = "linux", feature = "deprecated-applets"))]
            $entry {
                name: "uptime",
                main: applets::uptime::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "watch",
                main: applets::watch::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "wc",
                main: applets::wc::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "wget",
                main: applets::wget::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "which",
                main: applets::which::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "whoami",
                main: applets::whoami::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "xargs",
                main: applets::xargs::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "xz",
                main: applets::xz::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "xzcat",
                main: applets::xz::main_xzcat,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "yes",
                main: applets::yes::main,
            },
            #[cfg(feature = "deprecated-applets")]
            $entry {
                name: "zcat",
                main: applets::gzip::main_zcat,
            },
        ]
    };
}
