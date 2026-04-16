# Tier 1 Stabilization

Tier 1 applets are implemented. This file tracks the remaining work to turn that breadth milestone into a production-worthy Linux base system.

Use this as an action list, not a narrative:

- `[x]` closed for Tier 1 stabilization
- `[ ]` still open
- section `Status` shows whether the workstream is active or waiting

## Exit Gate

- [x] Tier 1 applets are implemented and registered.
- [ ] Tier 1 critical-path applets have no known reduced-scope gaps that would surprise an Alpine/Linux distro user in normal boot, install, recovery, networking, or admin flows.
- [ ] `APPLETS.md` is narrowed to intentional limitations, not accidental gaps.
- [x] A repeatable Tier 1 stabilization gate exists.
- [x] The gate covers unprivileged Alpine build, tests, and clippy.
- [x] The gate covers BusyBox-style shell coverage.
- [x] The gate covers privileged Linux integration coverage for PID 1 and capability-heavy paths.
- [ ] Parser/interpreter applets with high regression risk have stronger differential and edge-case coverage.
- [ ] Shared Linux-facing helpers have a cleaner separation between live system behavior and test scaffolding.
- [x] Support expectations are explicit for parity targets, intentionally smaller applets, and out-of-scope behavior.

## Current Priority

### P0

- [ ] Close the remaining reduced-scope gaps in `APPLETS.md` for critical Linux applets.
- [ ] Finish differential and edge-case hardening for `awk`.
- [ ] Finish differential and edge-case hardening for `sed`.
- [ ] Add stronger malformed-input and partial-write coverage for archive/compression applets.

### P1

- [x] Decide and document the Tier 1 support contract for `getty`, `login`, `su`, `sulogin`, `halt`, `poweroff`, and `reboot`.
- [ ] Split high-risk mixed applets only where that materially lowers regression risk.
- [ ] Separate live Linux backends from state-backed test scaffolding in shared helpers.
- [ ] Improve failure-path coverage and internal error classification in risky codepaths.

## Scrutiny Guide

- `Critical`
  - boot/auth, mounts, block devices, destructive filesystem changes, package/data integrity, or system state mutation
  - requires adversarial tests, malformed-input coverage, failure-path checks, and Linux integration tests
- `High`
  - parser-heavy or script-facing tools where subtle drift breaks installers, builds, or shell scripts
  - requires broad flag coverage and differential or host-oracle testing where behavior is stable
- `Routine`
  - useful but failures are usually localized and obvious
  - needs solid tests, but not the same adversarial depth
- `Low`
  - trivial or obvious behavior
  - a few focused tests are enough

Default choices:

- `Critical` for data-destructive, system-mutating, or package/install-path applets
- `High` for parser-heavy or script-foundation tools
- `Low` only for simple output/transformation applets

## Support and Risk Matrix

### Critical Parity Targets

- boot, storage, recovery
  - `init`, `mount`, `umount`, `fsck`, `blkid`, `fdisk`, `mkswap`, `swapon`, `swapoff`, `pivot_root`, `switch_root`, `chroot`
- auth and session
  - `login`, `passwd`, `su`, `sulogin`, `getty`, `halt`, `poweroff`, `reboot`
- package and data integrity
  - `tar`, `cpio`, `unzip`, compression applets, checksum applets, `cp`, `mv`, `rm`, `ln`, `install`, `dd`
- system state and kernel/device management
  - `mdev`, `losetup`, `modprobe`, `depmod`, `insmod`, `rmmod`, `lsmod`

### High Parity Targets

- parser and script foundation
  - `awk`, `sed`, `grep`, `egrep`, `fgrep`, `xargs`, `find`, `test`, `[`, `[[`, `expr`, `sort`, `cut`
- script-visible environment and metadata
  - `head`, `tail`, `stat`, `df`, `du`, `ls`, `date`, `env`, `readlink`, `realpath`, `nohup`, `timeout`
- networking admin and bootstrap
  - `ip`, `ifconfig`, `netstat`, `ping`, `ping6`, `udhcpc`, `udhcpd`, `ntpd`, `hostname`

### Routine Targets

- process and observability
  - `ps`, `pgrep`, `pkill`, `kill`, `killall`, `free`, `uptime`, `watch`
- admin helpers
  - `nslookup`, `wget`, `which`, `run-parts`, `sysctl`, `flock`, `setsid`, `less`, `man`, `tree`

### Low-Risk Targets

- simple output/transformation tools
  - `yes`, `true`, `false`, `whoami`, `pwd`, `printenv`, `basename`, `dirname`, `nproc`, `tty`, `seq`, `rev`, `sleep`, `echo`

## Workstreams

### P0: Release Gate

Status: `in progress`

- [x] Add a canonical Tier 1 stabilization runner.
- [x] Make Alpine the default oracle for Linux-only applets.
- [x] Keep privileged Linux integration checks in the same gate.
- [ ] Record explicit pass/fail expectations for each gate phase so regressions are attributable quickly.
- [x] Keep the unprivileged Alpine quick gate green.
- [x] Keep the full Tier 1 gate green.

Current gate:

- `bin/alpine-stabilization --quick`
  - `cargo test --quiet`
  - `cargo clippy --all-targets --all-features --quiet -- -D warnings`
  - `tests/run-applet-tests.sh`
- `bin/alpine-stabilization`
  - quick gate
  - `tests/run-linux-init.sh`
  - `tests/run-linux-phase7b.sh --privileged`

### P0: Boot, Storage, and Recovery Closure

Status: `in progress`

- [x] `mount`: document the supported helper-fallback contract and keep broader libmount-style orchestration out of Tier 1 scope.
- [x] `fsck`: support `-T` title suppression and make same-pass `-P` scheduling the explicit Tier 1 model.
- [x] `blkid`: expand direct probe coverage to the installer/storage matrix the distro actually needs.
- [ ] `fdisk`: decide whether DOS extended/logical partitions are required for installer or recovery flows.
- [x] `init`: keep PID 1 hardening coverage in the release gate.

### P0: Networking and Module Stack Closure

Status: `in progress`

- [x] `udhcpc -a`: validate DHCPACK addresses, send `DHCPDECLINE` on conflict, and retry acquisition.
- [x] `udhcpd`: honor `DHCPDECLINE` with `decline_time` and reoffer the next address.
- [x] `udhcpc` / `udhcpd`: preserve BOOTP `siaddr`, `sname`, and `boot_file` end to end.
- [x] `ip` / `netstat`: add family-aware `-4` / `-6` display paths with IPv6 route coverage.
- [x] `ip`: cover BusyBox-style global `-f inet|inet6|link` and `-o`.
- [x] `ip`: cover `addr flush`, `neigh show/flush`, `rule list/add/del`, and `route replace/change/flush`.
- [x] `ip`: cover higher-value route attributes and `link set` admin flags.
- [x] `netstat -p`: resolve live sockets back to `PID/Program name`.
- [x] `ping` / `ping6`: cover `-I`, `-i`, `-w`, and `-t`.
- [x] `ifconfig`: cover classic admin flags, `metric`, `txqueuelen`, `mem_start`, `io_addr`, and `irq`.
- [x] `ifconfig`: cover state-backed `add` / `del` address handling, including IPv6.
- [x] `ip` / `ifconfig`: add privileged Linux coverage for live IPv6 address mutation.
- [x] `ip`: add privileged Linux coverage for live IPv4/IPv6 route mutation, including route attributes.
- [x] `ip` / `ifconfig` / `netstat` / `ping`: document the intended Tier 1 admin subset in `APPLETS.md`.
- [ ] `modprobe`: close config parsing edge cases and test module policy behavior more aggressively.
- [x] `ntpd`: keep the current small authenticated/bootstrap daemon as the intended Tier 1 clock-sync surface.

### P0: Differential and Edge-Case Coverage

Status: `open`

- [ ] `awk`: differential test the supported language surface against BusyBox.
- [ ] `awk`: close or explicitly document unsupported forms that remain in Tier 1 scope.
- [ ] `sed`: differential test nested blocks, range behavior, and NUL/regex edges more aggressively.
- [ ] archive/compression applets: add malformed, truncated, and partial-write coverage where the suite is still light.

### P1: Session and Account Stack

Status: `closed`

- [x] `getty`: keep Tier 1 scoped to prompt/banner/login handoff rather than full tty line-discipline configuration.
- [x] `login` / `su` / `sulogin`: keep PAM out of scope for Tier 1.
- [x] Document the non-PAM support contract clearly in `APPLETS.md`.
- [x] `halt` / `poweroff` / `reboot`: keep `-w` as an intentional success no-op without utmp/wtmp writes.

### P1: Architectural Cleanup

Status: `open`

- [ ] `awk`: split parse, eval, runtime, and builtins only if that materially lowers regression risk.
- [ ] `sed`: split parse, execution, regex/text helpers, and file-I/O policy only if that materially lowers regression risk.
- [ ] Separate live Linux backends from state-file test backends in shared helpers such as `common/net`.
- [ ] Keep cleanup narrow and behavior-preserving.

### P1: Error Model and Diagnostics

Status: `open`

- [ ] Preserve BusyBox-style stderr output for users.
- [ ] Improve internal error classification enough to make risky codepaths easier to audit and test.
- [ ] Add explicit failure-path tests for syscalls, partial writes, unreadable inputs, and malformed config.

## `APPLETS.md` Gaps Still Open

These are the open items that still appear to be accidental gaps or unresolved scope decisions rather than clearly intentional limitations.

- [ ] `sed`: NUL-input regex behavior, nested-block execution corners, and some range behavior inside nested blocks.
- [ ] `awk`: `do ... while`, missing `getline` variants, `nextfile`, and deeper regex/substitution edges need either implementation or explicit scope decisions.
- [ ] `fdisk`: DOS extended/logical partitions remain unsupported.

## Next Up

The most defensible next stabilization tranche is:

1. [ ] Convert `awk` and `sed` hardening into explicit differential test backlogs, then land the first failing cases.
2. [x] Decide and document the Tier 1 support surface for `ip`, `ifconfig`, `netstat`, `ping`, and `ntpd` so `APPLETS.md` stops mixing intentional scope with unfinished work.
3. [x] Close the most concrete critical-path Linux gaps: `fsck -T`, `blkid` probe coverage, and the `mount` fallback story.
4. [x] Resolve the session/account support contract: PAM, utmp/wtmp, tty/session management, and reboot-family `-w`.

## Recent Closures

### 2026-04-15

- [x] Turned `APPLETS.md` from implicit scope notes into explicit Tier 1 support contracts for `mount`, `fsck`, `blkid`, `ntpd`, `ip`, `ifconfig`, `netstat`, `ping` / `ping6`, `getty`, `login`, `su`, `sulogin`, `halt`, `poweroff`, and `reboot`.
- [x] Corrected the tracker to refer to the actual reboot-family applets in the tree instead of a nonexistent `shutdown` applet.
- [x] Implemented `fsck -T` as real banner suppression and added BusyBox-style shell coverage for the title/no-title paths.
- [x] Expanded `blkid` direct probe coverage with `iso9660` and `squashfs`, with both unit coverage and BusyBox-style shell tests.
- [x] Closed the Tier 1 support-contract questions for non-PAM auth/session behavior, tty/login handoff, and reboot-family `-w`.

### 2026-04-14

- [x] Added the initial Tier 1 stabilization gate: `tests/run-tier1-stabilization.sh` and `bin/alpine-stabilization`.
- [x] Fixed a false-red `man` unit test by decoupling it from `/usr/bin/man` existing in Alpine.
- [x] Fixed `tests/run-applet-tests.sh` to honor `CARGO_TARGET_DIR`, so Alpine shell tests use the Linux binary instead of the host build.
- [x] Fixed `sed` so undefined branch labels are rejected at parse time, matching the shell regression case on empty input.
- [x] Fixed `patch` unit tests to avoid process-wide `cwd` mutation under parallel test execution.
- [x] Fixed multiple process-tool exit-status and matching bugs in `pgrep`, `pkill`, `pidof`, and `killall`.
- [x] Fixed `printf` missing-argument handling so `xargs` batch behavior now matches BusyBox for `%s`.
- [x] Fixed `dd` sparse-output and suffix parsing behavior, which unblocked `mkswap` and large-file fixture setup.
- [x] Hardened `tar` overwrite and hardlink handling, including preserved hardlinks across overwrite and hardlinked symlink entries.
- [x] Tightened `ls` block accounting and long-format rendering to match the Alpine `/bin/ls` oracle more closely.
- [x] Converted several shell tests to use explicit system-tool oracles where the previous failures were in the oracle applet rather than the applet under test.
- [x] Cleared the unprivileged Alpine quick gate end to end.
- [x] Cleared the full Tier 1 stabilization gate end to end, including the privileged PID 1 and phase7b checks.
- [x] Closed the `udhcpc -a` gap with DHCPACK ARP validation, `DHCPDECLINE`, and reacquisition coverage.
- [x] Closed the `udhcpd` decline-handling gap so the built-in server now respects client-side ARP conflict detection.
- [x] Wired BOOTP `siaddr`, `sname`, and `boot_file` through `udhcpd` replies and `udhcpc` script env export, with a new end-to-end shell test.
- [x] Added family-aware `ip -4/-6` and `netstat -4/-6` coverage with IPv6 route display and state-backed IPv6 route mutation tests.
- [x] Added `netstat -p` with live inode-to-process resolution and a shell test against a real listening Python socket.
- [x] Widened `ip` with precise IPv4 add/del semantics, `addr flush`, `route replace/change/flush`, and state-backed coverage for the new control paths.
- [x] Widened `ifconfig` with classic flag toggles plus `metric` and `txqueuelen`, with state-backed shell coverage.
- [x] Widened `ifconfig` with state-backed address `add` / `del` coverage, including IPv6.
- [x] Widened `ping` / `ping6` with `-I`, `-i`, `-w`, and `-t`, plus loopback interface-binding coverage.
- [x] Added a real rtnetlink-backed live IPv6 mutation path for `ip` / `ifconfig`, with privileged Linux coverage for loopback address and route changes.
- [x] Widened `ifconfig` with `mem_start`, `io_addr`, and `irq`, with state-backed coverage for the old hardware-map knobs.
- [x] Widened `ip route` with `src`, `metric`, `table`, and `proto`, moved live route listing/mutation onto the rtnetlink path, and added both state-backed and privileged Linux coverage for the new route attributes.
- [x] Added `ip neigh show|flush` with `to`, `dev`, and `nud` filtering, backed by both the state test backend and live rtnetlink neighbor dumps/deletes.
- [x] Added `ip rule list|add|del` with `from`, `to`, `iif`, `oif`, `fwmark`, `priority`, and `lookup/table`, backed by both the state test backend and live rtnetlink rule dumps/mutation.
- [x] Added BusyBox-style global `ip -f FAMILY` parsing and `-o` acceptance, with focused `inet6 route` and parser coverage.
