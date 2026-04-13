# Applets TODO

Implemented applets (registered in `src/lib.rs`) are checked off.

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
- [ ] unzip

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
- [ ] chgrp
- [ ] chown
- [ ] chroot
- [ ] dd
- [ ] df
- [ ] du
- [ ] expr
- [ ] install
- [x] mkfifo
- [ ] split
- [ ] stat
- [ ] stty
- [ ] test / [ / [[
- [ ] tree

## Editors

- [x] diff
- [ ] awk
- [x] cmp
- [ ] hexdump
- [ ] patch
- [ ] sed

## Finding Utilities

- [x] grep
- [x] egrep
- [x] fgrep
- [x] xargs

## Init Utilities

- [ ] halt
- [ ] init
- [ ] poweroff
- [ ] reboot

## Login / Password Management

- [ ] addgroup
- [ ] adduser
- [ ] delgroup
- [ ] deluser
- [ ] getty
- [ ] login
- [ ] nologin
- [ ] passwd
- [ ] su
- [ ] sulogin

## Linux Ext2 FS Progs

- [ ] fsck

## Linux Module Utilities

- [ ] depmod
- [ ] insmod
- [ ] lsmod
- [ ] modinfo
- [ ] modprobe
- [ ] rmmod

## Linux System Utilities

- [x] losetup
- [ ] acpid
- [ ] blkid
- [ ] dmesg
- [ ] fdisk
- [ ] flock
- [ ] getopt
- [ ] hwclock
- [ ] mdev
- [ ] mkswap
- [ ] mount
- [ ] mountpoint
- [ ] pivot_root
- [ ] setsid
- [ ] swapoff
- [ ] swapon
- [ ] switch_root
- [ ] sysctl
- [ ] umount

## Miscellaneous Utilities

- [ ] less
- [ ] lsattr / chattr
- [ ] man
- [ ] run-parts
- [ ] time
- [x] which

## Networking Utilities

- [x] wget
- [x] hostname
- [ ] ifconfig
- [ ] ip
- [ ] netstat
- [ ] nslookup
- [ ] ntpd
- [ ] ping / ping6
- [ ] rfkill
- [ ] udhcpc
- [ ] udhcpd

## Process Utilities

- [x] kill
- [ ] free
- [ ] killall
- [ ] killall5
- [ ] lsof
- [ ] pgrep
- [ ] pkill
- [ ] ps
- [ ] uptime
- [ ] watch

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
