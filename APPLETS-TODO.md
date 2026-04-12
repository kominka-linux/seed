# Applets TODO

Applets enabled in `busybox-config` that remain to be implemented.
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
- [ ] base32
- [ ] base64
- [ ] chgrp
- [ ] chown
- [ ] chroot
- [ ] cksum
- [ ] crc32
- [ ] dd
- [ ] df
- [ ] du
- [ ] expr
- [ ] groups
- [ ] id
- [ ] install
- [x] link
- [x] ln
- [ ] md5sum / sha1sum / sha256sum / sha512sum
- [ ] mkfifo
- [ ] nice
- [x] paste
- [x] readlink
- [x] realpath
- [ ] rev
- [ ] shuf
- [ ] split
- [ ] stat
- [ ] strings
- [ ] stty
- [x] sync / fsync
- [ ] test / [ / [[
- [x] timeout
- [x] touch
- [x] tr
- [ ] tree
- [ ] usleep

## Console Utilities

- [ ] clear
- [ ] reset
- [ ] resize

## Editors

- [x] diff
- [ ] awk
- [ ] cmp
- [ ] ed
- [ ] hexdump
- [ ] hexedit
- [ ] patch
- [ ] sed
- [ ] vi

## Finding Utilities

- [x] grep
- [x] egrep
- [x] fgrep
- [ ] xargs

## Init Utilities

- [ ] halt
- [ ] init
- [ ] poweroff
- [ ] reboot

## Login / Password Management

- [ ] add-shell
- [ ] addgroup
- [ ] adduser
- [ ] chpasswd
- [ ] cryptpw
- [ ] delgroup
- [ ] deluser
- [ ] getty
- [ ] login
- [ ] mkpasswd
- [ ] passwd
- [ ] remove-shell
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
- [ ] crond
- [ ] crontab
- [ ] dmesg
- [ ] fallocate
- [ ] fdisk
- [ ] flock
- [ ] fstrim
- [ ] getopt
- [ ] hwclock
- [ ] logger
- [ ] mdev
- [ ] mkswap
- [ ] mount
- [ ] mountpoint
- [ ] nsenter
- [ ] nologin
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
- [ ] lspci
- [ ] lsscsi
- [ ] lsusb
- [ ] man
- [ ] renice
- [ ] rfkill
- [ ] run-parts
- [ ] script
- [ ] scriptreplay
- [ ] seedrng
- [ ] setpriv
- [ ] shred
- [ ] time
- [ ] ts
- [ ] tsort
- [ ] unshare
- [ ] which

## Networking Utilities

- [x] wget
- [x] hostname
- [ ] arp
- [ ] brctl
- [ ] ifconfig
- [ ] ip
- [ ] netstat
- [ ] nslookup
- [ ] ntpd
- [ ] ping / ping6
- [ ] route
- [ ] tc
- [ ] traceroute / traceroute6
- [ ] udhcpc
- [ ] udhcpd

## Process Utilities

- [ ] chrt
- [ ] free
- [ ] fuser
- [ ] ionice
- [ ] kill
- [ ] killall
- [ ] killall5
- [ ] lsof
- [ ] pgrep
- [ ] pidof
- [ ] pkill
- [ ] ps
- [ ] pstree
- [ ] taskset
- [ ] top
- [ ] uptime
- [ ] watch

## Shells

- [ ] cttyhack

## System Logging Utilities

- [ ] klogd
- [ ] logread
- [ ] syslogd

## Won't Do

- ash — ysh is the system shell
- ssl_client — curl handles TLS
- uevent — disabled from busybox; mdev covers device events
- Runit utilities (runsv, runsvdir, sv, svc, svlogd, svok, chpst,
  envdir, envuidgid, setuidgid, softlimit) — provided by the separate
  runit package; implementing in seed would conflict
