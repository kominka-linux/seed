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
  - Tier 1 support contract:
    regular mounts use the direct `mount(2)` path, with explicit `mount.<fstype>` helper fallback for ordinary filesystem mounts when a helper exists
    `-a`, fstab filtering, `UUID=`/`LABEL=` resolution, bind/move/remount, and propagation flags are part of the supported surface
  - Intentional limits:
    no broader libmount-style orchestration, loop setup, probing, or helper policy beyond explicit `mount.<fstype>` fallback
- `fsck`
  - Tier 1 support contract:
    delegated `fsck.<type>` execution, `-A`, `-N`, `-P`, `-R`, `-t`, `-V`, and `-T` title suppression are supported
  - Intentional limits:
    same-pass `-P` parallelism is the supported model; there is no richer pass scheduling or device-topology awareness
- `ntpd`
  - Tier 1 support contract:
    one-shot sync/query (`-q`, `-w`), explicit peers (`-p`), config-file peers, keyed MD5/SHA1 peers via key files, post-sync scripts (`-S`), simple listen mode (`-l`), and interface binding (`-I`) are supported
    this is the intended bootstrap/base-system clock-sync surface for Tier 1
  - Intentional limits:
    not a full long-running clock-disciplining daemon; no drift file, refclock, broadcast, or broader ntpd tuning/config surface
- `blkid`
  - Tier 1 support contract:
    direct probes cover the installer/storage matrix used by the base system: `ext2/3/4`, `xfs`, `btrfs`, `vfat`, `swap`, `iso9660`, and `squashfs`
    cache-backed listing and `/dev/disk/by-*` tag discovery remain supported
  - Intentional limits:
    still not a full libblkid-style probe matrix
- `udhcpc`
- `udhcpd`
  - handles `DHCPDECLINE` with `decline_time`
  - now serves and exports BOOTP `siaddr`, `sname`, and `boot_file` for client-script flows
  - config surface is still a BusyBox-style subset
  - several common directives are accepted and ignored
- `ip`
  - Tier 1 support contract:
    `addr`, `link`, `route`, `neigh`, and `rule` are the supported object families
    `-4`, `-6`, global `-f inet|inet6|link`, and `-o` are supported
    `addr` covers `show`, `add`, `del`, and `flush`; `link` covers `show` and `set`; `route` covers `show`, `add`, `append`, `del`, `change`, `replace`, and `flush`; `neigh` covers `show` and `flush`; `rule` covers `list`, `add`, and `del`
    route attributes (`src`, `metric`, `table`, `proto`) and the current link-admin flags are part of the intended Tier 1 surface
  - Intentional limits:
    no `tunnel` support or broader `iproute2` object families
- `ifconfig`
  - Tier 1 support contract:
    display includes IPv4 and IPv6 addresses
    IPv4 primary address changes plus IPv4/IPv6 `add` / `del` are supported
    classic admin flags cover `up` / `down`, `arp`, `allmulti`, `multicast`, `promisc`, `dynamic`, and `trailers`
    `mtu`, `metric`, `txqueuelen`, `mem_start`, `io_addr`, `irq`, and `hw ether` are supported
  - Intentional limits:
    link mutation remains Ethernet-oriented
    only `hw ether` is supported, and older niche knobs such as `outfill` and `keepalive` remain unsupported
- `netstat`
  - Tier 1 support contract:
    IPv4/IPv6 TCP and UDP socket listing plus route display are supported
    `-l`, `-n`, `-t`, `-u`, `-r`, `-4`, `-6`, `-W`, and `-p` are the intended Tier 1 option subset
  - Intentional limits:
    no broader netstat families or reporting modes beyond the current internet-socket and route subset
- `ping` / `ping6`
  - Tier 1 support contract:
    basic ICMP echo for IPv4/IPv6 with `-4`, `-6`, `-c`, `-W`, `-w`, `-i`, `-s`, `-q`, `-I`, and `-t`
    this is the intended Tier 1 surface for interface binding, timeout/deadline handling, and basic reachability checks
  - Intentional limits:
    still a minimal echo implementation; many traditional ping diagnostics remain out of scope
- `acpid`
  - several compatibility flags are accepted and ignored
  - evdev handling remains intentionally narrow
- `halt` / `poweroff` / `reboot`
  - Tier 1 support contract:
    `-d`, `-n`, `-f`, and `-w` are supported
    non-`-f` behavior is the intended signal-based PID 1 handoff, and `-f` uses the direct reboot syscall path
  - Intentional limits:
    `-w` is an intentional success no-op for compatibility; there are no utmp/wtmp writes
- `hwclock`
  - `--param-*` access is still unsupported
- `getty`
  - Tier 1 support contract:
    issue display, login prompt, optional init string output, custom login applet selection, host forwarding, and TERM forwarding are supported
  - Intentional limits:
    baud/tty operands are parsed for invocation compatibility, but line settings are not fully applied
    several traditional compatibility flags are accepted and ignored rather than implemented
- `login`
  - Tier 1 support contract:
    local `/etc/passwd` + `/etc/shadow` authentication via `crypt(3)`, `-f`, `-p`, `-h`, login-shell exec, and the current HOME/SHELL/USER/LOGNAME/PATH environment setup are the supported surface
  - Intentional limits:
    no PAM, utmp/wtmp session accounting, supplementary-group/session stack setup, or fuller TTY/login-record management
- `su`
  - Tier 1 support contract:
    local `/etc/shadow` authentication when required, `-`, `-l`, `-m` / `-p`, `-s`, and `-c` are supported for local admin and recovery flows
  - Intentional limits:
    no PAM, utmp/wtmp/session stack, or supplementary-group/session initialization
- `sulogin`
  - Tier 1 support contract:
    root maintenance-shell auth, EOF-to-continue behavior, and `-e` forced shell behavior for locked/missing root hashes are the intended support surface
  - Intentional limits:
    intentionally a small non-PAM maintenance path, without broader session or tty management
- `fdisk`
  - DOS extended/logical partitions remain unsupported
  - `-u/-C/-H/-S` are still mostly compatibility surface
- `lsattr` / `chattr`
  - real ioctl path exists
  - deterministic test backend is still used where filesystems do not support the real ext ioctls
