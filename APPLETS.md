# Applet Notes

This file tracks applet-specific limitations that are important to keep visible even when the broader backlog moves around.

## sed

- Current state: substantially expanded BusyBox-style `sed` with in-place edits, ranges, branching, hold space operations, translation, and a broader command set than the initial implementation.
- Known limitations:
  - not yet claimed as full BusyBox parity
  - regex behavior on NUL-containing input is still a known weak edge
  - some nested-block execution corners are still suspect, especially `n` inside `{ ... }`
  - some range-address behavior inside nested blocks may still drift from BusyBox

## awk

- Current state: broad BusyBox-style `awk` subset with arrays, functions, regex literals, pattern rules, `for`/`while`, `next`, `exit`, `getline <file`, `sub`/`gsub`, `split`, `match`, `index`, `substr`, `sprintf`, `close`, and the covered special cases in `tests/busybox/awk`.
- Known limitations:
  - not yet claimed as full BusyBox parity
  - `do ... while` is still unsupported
  - only a subset of `getline` forms is implemented; stdin/pipe variants are still missing
  - `nextfile` is still unsupported
  - deeper regex and substitution edge cases likely remain outside the current coverage

## Reduced-Scope Linux Applets

- `mount`
  - helper fallback is present, but the helper path is still narrow
  - bind/move/remount/propagation stay on the direct syscall path
  - no broader userspace orchestration beyond `mount.<fstype>` fallback
- `fsck`
  - same-pass `-P` parallelism exists
  - `-T` is still parsed but unused
  - no richer pass scheduling or device-topology awareness
- `ntpd`
  - keyed peers and key files exist
  - still much smaller than a full clock-disciplining daemon
  - config and polling/discipline behavior remain intentionally narrow
- `blkid`
  - direct probes now cover common filesystems
  - still not a full libblkid-style probe matrix
- `udhcpc`
- `udhcpd`
  - handles `DHCPDECLINE` with `decline_time`
  - now serves and exports BOOTP `siaddr`, `sname`, and `boot_file` for client-script flows
  - config surface is still a BusyBox-style subset
  - several common directives are accepted and ignored
- `ip`
  - family-aware `-4` / `-6` display exists for `addr` and `route`
  - `addr` now covers `show`, `add`, `del`, and `flush` with `dev` / `to` / `label` / `scope` filtering
  - `neigh` now covers `show` and `flush` with `to` / `dev` / `nud` filtering
  - `rule` now covers `list`, `add`, and `del` with `from`, `to`, `iif`, `oif`, `fwmark`, `priority`, and `lookup/table`
  - `route` now covers `show`, `add`, `append`, `del`, `change`, `replace`, and `flush`, including `src`, `metric`, `table`, and `proto`
  - `link set` now covers `up` / `down`, `arp`, `multicast`, `allmulti`, `promisc`, `mtu`, `qlen`, `name`, and `address`
  - still much smaller than full BusyBox `ip`
  - no `tunnel` or broader `iproute2`-style object families
- `ifconfig`
  - display includes IPv4 and IPv6 addresses
  - IPv6 address `add` / `del` now works against the live kernel path too
  - classic link flags now include `arp`, `allmulti`, `multicast`, `promisc`, `metric`, and `txqueuelen`
  - hardware-map knobs now include `mem_start`, `io_addr`, and `irq`
  - link mutation remains Ethernet-oriented
  - only `hw ether` is supported
  - older or niche knobs like `trailers`, `dynamic`, `outfill`, and `keepalive` are still missing
- `netstat`
  - family-aware socket and route display exists
  - process display via `-p` now resolves socket inodes back to `/proc`
  - still only the current small option subset
- `ping` / `ping6`
  - supports `-I`, `-i`, `-w`, and `-t`
  - still a minimal ICMP echo implementation
  - many traditional ping behaviors remain absent
- `acpid`
  - several compatibility flags are accepted and ignored
  - evdev handling remains intentionally narrow
- `shutdown` / `halt` / `poweroff` / `reboot`
  - `-w` is still a success no-op without utmp/wtmp writes
  - non-`-f` behavior is still signal-based init handoff
- `hwclock`
  - `--param-*` access is still unsupported
- `getty`
  - baud and tty parsing exists, but line settings are not fully applied
  - several traditional flags are accepted and ignored
- `login`
  - no PAM
  - no utmp/wtmp session accounting
  - no fuller TTY/session management
- `su`
  - no PAM
  - no utmp/wtmp/session stack
- `sulogin`
  - still a small maintenance-shell/auth path
- `fdisk`
  - DOS extended/logical partitions remain unsupported
  - `-u/-C/-H/-S` are still mostly compatibility surface
- `lsattr` / `chattr`
  - real ioctl path exists
  - deterministic test backend is still used where filesystems do not support the real ext ioctls
