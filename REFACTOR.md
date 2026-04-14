# Refactor Backlog

This file tracks Linux-only applets that were intentionally implemented as reduced-scope versions and need follow-up work for distro-quality compatibility.

## Progress

- Completed in this pass:
  - `mount`: helper fallback plus `-i` helper suppression
  - `fsck`: real same-pass parallel execution for `-P`
  - `ntpd`: key-file auth, keyed peers, broader config parsing
  - `blkid`: direct probes for `ext*`, `vfat`, `xfs`, and `btrfs`

## Priority 0: Installer / Boot / Core Runtime

- `mount`
  - Current state: direct `mount(2)` wrapper with `fstab` support plus `mount.<fstype>` helper fallback.
  - Gaps:
    - helper path is still narrow and only covers explicit `mount.<fstype>` binaries
    - bind/move/remount/propagation operations intentionally stay on the direct syscall path
    - no broader userspace orchestration beyond the helper fallback
  - Source: `src/applets/mount.rs`

- `fsck`
  - Current state: delegates to `fsck.<type>` helpers and can run same-pass checks in parallel with `-P`.
  - Gaps:
    - `-T` parsed but unused
    - no richer pass scheduling than grouping by `passno`
    - no device-topology awareness to avoid conflicting parallel checks
  - Source: `src/applets/fsck.rs`

- `ntpd`
  - Current state: query/step/listen loop with keyed peers, key-file loading, and broader `ntp.conf` parsing.
  - Gaps:
    - not a full clock-disciplining daemon
    - config surface is still intentionally small beyond `server` / `peer` / `keys` / `trustedkey`
    - polling/discipline behavior is still much smaller than a full `ntpd`
  - Source: `src/applets/ntpd.rs`

- `blkid`
  - Current state: discovers tags from `/dev/disk/by-*`, mountinfo, swap headers, and direct superblock probes for common filesystems.
  - Gaps:
    - still not a full libblkid-style probe matrix
    - direct probing is currently focused on swap, `ext*`, `vfat`, `xfs`, and basic `btrfs`
  - Source: `src/applets/blkid.rs`

## Priority 1: Networking / Machine Management

- `udhcpc`
  - Gap: `-a` ARP offer validation unsupported.
  - Source: `src/applets/udhcpc.rs`

- `udhcpd`
  - Gaps:
    - config surface is a BusyBox-style subset
    - several common directives are accepted and ignored
  - Source: `src/applets/udhcpd.rs`

- `ip`
  - Current state: narrow `addr`, `link`, and `route` subset.
  - Gaps:
    - many `ip` subcommands absent
    - route mutation is IPv4-only
  - Source: `src/applets/ip.rs`, `src/common/net.rs`

- `ifconfig`
  - Current state: IPv4/Ethernet subset.
  - Gaps:
    - only `hw ether`
    - no broader family/media/tunnel coverage
  - Source: `src/applets/ifconfig.rs`

- `netstat`
  - Current state: `/proc/net/*` parser for a small option subset.
  - Gaps:
    - only `-a -l -n -t -u -r -W`
    - route output is IPv4-only
  - Source: `src/applets/netstat.rs`

- `ping` / `ping6`
  - Current state: minimal ICMP echo implementation.
  - Gaps:
    - only `-4/-6/-c/-W/-s/-q`
    - many traditional ping behaviors absent
  - Source: `src/applets/ping.rs`

- `acpid`
  - Current state: text event reader plus narrow evdev mapping path.
  - Gaps:
    - several compatibility flags accepted and ignored
    - evdev handling is intentionally narrow
  - Source: `src/applets/acpid.rs`

- `shutdown` / `halt` / `poweroff` / `reboot`
  - Gaps:
    - `-w` is a success no-op and does not write utmp/wtmp
    - non-`-f` behavior is only signal-based init handoff
  - Source: `src/applets/shutdown.rs`

- `hwclock`
  - Gap: `--param-*` RTC parameter access unsupported.
  - Source: `src/applets/hwclock.rs`

## Priority 2: Session / Recovery Stack

- `getty`
  - Gaps:
    - parses baud/tty but does not actually apply line settings
    - several traditional flags accepted and ignored
  - Source: `src/applets/getty.rs`

- `login`
  - Gaps:
    - no PAM
    - no utmp/wtmp session accounting
    - no fuller TTY/session management
  - Source: `src/applets/login.rs`

- `su`
  - Gaps:
    - no PAM
    - no utmp/wtmp/session stack
  - Source: `src/applets/su.rs`

- `sulogin`
  - Current state: small maintenance shell/auth path.
  - Gaps:
    - no fuller recovery/session handling
  - Source: `src/applets/sulogin.rs`

## Priority 3: Filesystem / Utility Completeness

- `fdisk`
  - Gaps:
    - DOS extended/logical partitions unsupported
    - `-u/-C/-H/-S` mostly compatibility surface, not full CHS behavior
  - Source: `src/applets/fdisk.rs`

- `lsattr` / `chattr`
  - Current state: real ioctl path plus deterministic state-file backend for tests.
  - Gaps:
    - test oracle is synthetic on filesystems where ext ioctls are unavailable
  - Source: `src/common/extattr.rs`

- `modprobe`
  - Current state: alias, blacklist, options, install/remove, and softdep support are present.
  - Gap:
    - config parsing strips `#` naively, so quoted `install` / `remove` fragments containing `#` are misparsed
  - Source: `src/common/modules.rs`
