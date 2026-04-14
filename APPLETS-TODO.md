# Applets TODO

Implemented applets (registered in `src/lib.rs`) are checked off.

Target platform note: this project targets Linux. Non-Linux hosts are only
editor/build hosts; applet behavior and implementation should be treated as
Linux-native.

## Scrutiny Guide

Use this to decide how much design and test effort an applet deserves.

- Critical: package/data integrity, destructive filesystem changes, boot/auth,
  mounts, block devices, or system state. These need adversarial tests,
  malformed-input coverage, failure-path checks, and Linux integration tests.
  Examples: `tar`, `patch`, `cpio`, `unzip`, compression applets, checksum
  applets, `cp`, `mv`, `rm`, `ln`, `install`, `dd`, `chmod`, `chown`,
  `chgrp`, `mount`, `umount`, `losetup`, `fsck`, `mkswap`, `swapon`,
  `swapoff`, `pivot_root`, `switch_root`, `chroot`, `init`, `halt`,
  `poweroff`, `reboot`, `login`, `passwd`, `su`, `sulogin`, `getty`, `mdev`.
- High: script foundation and text-processing behavior where subtle drift
  breaks builds, installers, or shell scripts. These need broad flag coverage
  and host-oracle tests where behavior is stable. Examples: `awk`, `sed`,
  `grep`, `egrep`, `fgrep`, `xargs`, `find`, `test`, `[`, `[[`, `expr`,
  `sort`, `cut`, `tee`, `head`, `tail`, `stat`, `df`, `du`, `ls`, `date`,
  `env`, `readlink`, `realpath`, `nohup`, `timeout`.
- Routine: useful but failures are usually localized and obvious. These still
  need solid tests, but not the same adversarial depth. Examples: `ps`,
  `pgrep`, `pkill`, `kill`, `killall`, `free`, `uptime`, `watch`,
  `hostname`, `nslookup`, `wget`, `which`, `run-parts`, `sysctl`, `flock`,
  `setsid`, `less`, `man`, `tree`.
- Low: trivial behavior or low consequence. Keep these simple and avoid
  over-engineering. A few focused tests are enough. Examples: `yes`, `true`,
  `false`, `whoami`, `pwd`, `printenv`, `basename`, `dirname`, `nproc`,
  `tty`, `seq`, `rev`, `sleep`, `echo`.

When in doubt: default to `Critical` for anything that can destroy data,
mutate system state, or sits in the package/install path; default to `High`
for parser-heavy or script-facing tools; default to `Low` only for pure,
obvious output utilities.

## Archival Utilities

- [x] bunzip2
- [x] bzip2
- [x] bzcat
- [x] gzip
- [x] gunzip
- [x] zcat
- [x] lzma
- [x] unlzma
- [x] lzcat
- [x] xz
- [x] unxz
- [x] xzcat
- [x] tar
- [ ] cpio
- [x] unzip

## Coreutils

- [x] basename
- [x] cat
- [x] chmod
- [x] cp
- [x] cut
- [x] date
- [x] dirname
- [x] echo
- [x] env
- [x] false
- [x] find
- [x] fold
- [x] head
- [x] ls
- [x] mkdir
- [x] mktemp
- [x] mknod
- [x] mv
- [x] nohup
- [x] nproc
- [x] od
- [x] printenv
- [x] printf
- [x] pwd
- [x] rm
- [x] rmdir
- [x] seq
- [x] sleep
- [x] sort
- [x] tail
- [x] tee
- [x] true
- [x] truncate
- [x] tty
- [x] uname
- [x] uniq
- [x] unlink
- [x] wc
- [x] whoami
- [x] yes
- [x] base32
- [x] base64
- [x] id
- [x] link
- [x] ln
- [x] md5sum / sha1sum / sha256sum / sha512sum
- [x] paste
- [x] readlink
- [x] realpath
- [x] rev
- [x] shuf
- [x] strings
- [x] sync / fsync
- [x] timeout
- [x] touch
- [x] tr
- [x] chgrp
- [x] chown
- [x] chroot
- [x] dd
- [x] df
- [x] du
- [x] expr
- [x] install
- [x] mkfifo
- [x] split
- [x] stat
- [ ] stty
- [x] test / [ / [[
- [x] tree

## Editors

- [x] diff
- [ ] awk
- [x] cmp
- [x] hexdump
- [x] patch
- [ ] sed

## Finding Utilities

- [x] grep
- [x] egrep
- [x] fgrep
- [x] xargs

## Init Utilities

- [x] halt
- [x] poweroff
- [x] reboot

## Login / Password Management

- [ ] addgroup
- [ ] adduser
- [ ] delgroup
- [ ] deluser
- [ ] getty
- [ ] login
- [x] nologin
- [ ] passwd
- [ ] su
- [ ] sulogin

## Linux Ext2 FS Progs

- [ ] fsck

## Linux Module Utilities

- [ ] depmod
- [x] insmod
- [x] lsmod
- [ ] modinfo
- [ ] modprobe
- [x] rmmod

## Linux System Utilities

- [x] losetup
- [ ] acpid
- [ ] blkid
- [x] dmesg
- [ ] fdisk
- [x] flock
- [x] getopt
- [ ] hwclock
- [ ] mdev
- [ ] mkswap
- [ ] mount
- [x] mountpoint
- [ ] pivot_root
- [x] setsid
- [ ] swapoff
- [ ] swapon
- [ ] switch_root
- [x] sysctl
- [ ] umount

## Miscellaneous Utilities

- [x] less
- [ ] lsattr / chattr
- [x] man
- [x] run-parts
- [x] time
- [x] which

## Networking Utilities

- [x] wget
- [x] hostname
- [ ] ifconfig
- [ ] ip
- [ ] netstat
- [x] nslookup
- [ ] ntpd
- [ ] ping / ping6
- [ ] rfkill
- [ ] udhcpc
- [ ] udhcpd

## Process Utilities

- [x] kill
- [x] free
- [x] killall
- [ ] killall5
- [x] lsof
- [x] pgrep
- [x] pkill
- [x] ps
- [x] uptime
- [x] watch

## System Logging Utilities

- [ ] klogd
- [ ] syslogd

---

## Tier 2

Useful for specific but not unusual tasks — implement after Tier 1.

### Coreutils

- [ ] cksum
- [ ] groups
- [ ] nice

### Console Utilities

- [ ] reset

### Login / Password Management

- [ ] chpasswd
- [ ] mkpasswd

### Linux System Utilities

- [ ] fallocate
- [ ] nsenter

### Miscellaneous Utilities

- [ ] lspci
- [ ] lsusb
- [ ] renice
- [ ] script
- [ ] shred
- [ ] unshare

### Networking Utilities

- [ ] arp
- [ ] brctl
- [ ] route
- [ ] tc
- [ ] traceroute / traceroute6

### Process Utilities

- [ ] chrt
- [ ] fuser
- [ ] ionice
- [ ] pidof
- [ ] pstree
- [ ] taskset

### System Logging Utilities

- [ ] logread

---

## Tier 3

Niche, hardware-specific, or very rarely needed.

### Console Utilities

- [ ] resize

### Linux System Utilities

- [ ] fstrim

---

## Won't Do

- ash — ysh is the system shell
- ssl_client — curl handles TLS
- uevent — disabled from busybox; mdev covers device events
- top — interactive TUI process viewer
- crond / crontab — cron provided by dedicated packages
- add-shell / remove-shell — too niche; /etc/shells rarely managed this way
- crc32 / cryptpw / scriptreplay / seedrng / setpriv / ts / tsort — too obscure or covered by other tools
- lsscsi — legacy SCSI hardware
- cttyhack — too specific to old busybox init TTY hacks
- usleep — sleep handles fractional seconds on modern systems
- Runit utilities (runsv, runsvdir, sv, svc, svlogd, svok, chpst,
  envdir, envuidgid, setuidgid, softlimit) — provided by the separate
  runit package; implementing in seed would conflict
