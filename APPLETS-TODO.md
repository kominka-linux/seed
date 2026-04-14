# Applets TODO

Implemented applets (registered in `src/lib.rs`) are checked off.

Target platform note: this project targets Linux. Non-Linux hosts are only
editor/build hosts; applet behavior and implementation should be treated as
Linux-native.

## Scrutiny Guide

Moved to [T1-STABILIZATION.md](/Users/josh/d/seed/T1-STABILIZATION.md:34), which now combines the scrutiny levels with the Tier 1 support/risk matrix and stabilization workstreams.

## Tier 1

### Archival Utilities

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
- [x] cpio
- [x] unzip

### Coreutils

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
- [x] stty
- [x] test / [ / [[
- [x] tree

### Editors

- [x] diff
- [x] awk
- [x] cmp
- [x] hexdump
- [x] patch
- [x] sed

### Finding Utilities

- [x] grep
- [x] egrep
- [x] fgrep
- [x] xargs

### Init Utilities

- [x] init
- [x] halt
- [x] poweroff
- [x] reboot

### Login / Password Management

- [x] addgroup
- [x] adduser
- [x] delgroup
- [x] deluser
- [x] getty
- [x] login
- [x] nologin
- [x] passwd
- [x] su
- [x] sulogin

### Linux Ext2 FS Progs

- [x] fsck

### Linux Module Utilities

- [x] depmod
- [x] insmod
- [x] lsmod
- [x] modinfo
- [x] modprobe
- [x] rmmod

### Linux System Utilities

- [x] losetup
- [x] acpid
- [x] blkid
- [x] dmesg
- [x] fdisk
- [x] flock
- [x] getopt
- [x] hwclock
- [x] mdev
- [x] mkswap
- [x] mount
- [x] mountpoint
- [x] pivot_root
- [x] setsid
- [x] swapoff
- [x] swapon
- [x] switch_root
- [x] sysctl
- [x] umount

### Miscellaneous Utilities

- [x] less
- [x] lsattr / chattr
- [x] man
- [x] run-parts
- [x] time
- [x] which

### Networking Utilities

- [x] wget
- [x] hostname
- [x] ifconfig
- [x] ip
- [x] netstat
- [x] nslookup
- [x] ntpd
- [x] ping / ping6
- [x] rfkill
- [x] udhcpc
- [x] udhcpd

### Process Utilities

- [x] kill
- [x] free
- [x] killall
- [x] killall5
- [x] lsof
- [x] pgrep
- [x] pkill
- [x] ps
- [x] uptime
- [x] watch

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
- [x] pidof
- [x] pstree
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
