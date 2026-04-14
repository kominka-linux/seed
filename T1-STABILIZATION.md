# Tier 1 Stabilization

This file tracks the work required to move `seed` from "Tier 1 breadth complete" to "production-worthy base system for Linux".

Tier 1 applets are implemented. The remaining gap is mostly hardening, compatibility closure, verification, and maintainability in the applets that matter most for boot, storage, networking, auth, and text-processing.

## Exit Criteria

- Tier 1 critical-path applets have no known reduced-scope gaps that would surprise an Alpine/Linux distro user in normal installer, boot, recovery, networking, or admin flows.
- `APPLETS.md` is narrowed to intentional limitations, not accidental gaps.
- Tier 1 has a repeatable stabilization gate that covers:
  - unprivileged Alpine build, tests, and clippy
  - BusyBox-style shell coverage
  - privileged Linux integration coverage for PID 1 and capability-heavy paths
- Parser/interpreter applets with high regression risk have stronger differential and edge-case coverage.
- Shared Linux-facing helpers have a cleaner separation between live system behavior and test scaffolding.
- Support expectations are explicit: which applets are parity targets, which are intentionally smaller, and which behaviors are out of scope.

## Current Risks

## P0

- System-critical Linux applets still have known compatibility holes in [APPLETS.md](/Users/josh/d/seed/APPLETS.md:24).
- There is not yet a single Tier 1 stabilization gate that treats the repo like a release candidate.
- `awk` and `sed` remain large mixed parser/runtime implementations, which raises audit and regression risk.
- Some Linux helpers still mix real system behavior with test-only state backends in the same modules.

## P1

- Session/login applets are intentionally minimal and need a clearer production target.
- Error reporting is BusyBox-friendly but thin for hardening and diagnostics.
- Some compatibility work is still implicit rather than tracked as explicit parity targets.

## Scrutiny Levels

Use these levels to decide how much design and test effort an applet deserves during stabilization.

- Critical
  - package or data integrity, destructive filesystem changes, boot/auth, mounts, block devices, or system state
  - requires adversarial tests, malformed-input coverage, failure-path checks, and Linux integration tests
- High
  - script foundation and text-processing behavior where subtle drift breaks builds, installers, or shell scripts
  - requires broad flag coverage and differential or host-oracle testing where behavior is stable
- Routine
  - useful but failures are usually localized and obvious
  - still needs solid tests, but not the same adversarial depth
- Low
  - trivial behavior or low consequence
  - a few focused tests are enough; avoid over-engineering

When in doubt:

- default to `Critical` for anything that can destroy data, mutate system state, or sits in the package/install path
- default to `High` for parser-heavy or script-facing tools
- default to `Low` only for pure, obvious output utilities

## Support and Risk Matrix

- Critical parity targets
  - boot, storage, recovery: `init`, `mount`, `umount`, `fsck`, `blkid`, `fdisk`, `mkswap`, `swapon`, `swapoff`, `pivot_root`, `switch_root`, `chroot`
  - auth and session: `login`, `passwd`, `su`, `sulogin`, `getty`, `shutdown`, `halt`, `poweroff`, `reboot`
  - package/data integrity: `tar`, `cpio`, `unzip`, compression applets, checksum applets, `cp`, `mv`, `rm`, `ln`, `install`, `dd`
  - system state and kernel/device management: `mdev`, `losetup`, `modprobe`, `depmod`, `insmod`, `rmmod`, `lsmod`
- High parity targets
  - parser and script foundation: `awk`, `sed`, `grep`, `egrep`, `fgrep`, `xargs`, `find`, `test`, `[`, `[[`, `expr`, `sort`, `cut`
  - script-visible environment and metadata: `head`, `tail`, `stat`, `df`, `du`, `ls`, `date`, `env`, `readlink`, `realpath`, `nohup`, `timeout`
  - networking admin and bootstrap: `ip`, `ifconfig`, `netstat`, `ping`, `ping6`, `udhcpc`, `udhcpd`, `ntpd`, `hostname`
- Routine stabilization targets
  - process and observability: `ps`, `pgrep`, `pkill`, `kill`, `killall`, `free`, `uptime`, `watch`
  - admin helpers: `nslookup`, `wget`, `which`, `run-parts`, `sysctl`, `flock`, `setsid`, `less`, `man`, `tree`
- Low-risk applets
  - simple output or transformation tools like `yes`, `true`, `false`, `whoami`, `pwd`, `printenv`, `basename`, `dirname`, `nproc`, `tty`, `seq`, `rev`, `sleep`, `echo`

## Workstreams

## P0: Release Gate

- Add and maintain one canonical Tier 1 stabilization runner.
- Make Alpine the default oracle for Linux-only applets.
- Keep privileged Linux integration checks in the same gate instead of as ad hoc follow-ups.
- Record pass/fail expectations for each phase so regressions are attributable quickly.

Status:
- In progress
- Initial gate runner added in this tranche: `tests/run-tier1-stabilization.sh` and `bin/alpine-stabilization`
- Unprivileged Alpine quick gate is now green:
  - `cargo test --quiet`
  - `cargo clippy --all-targets --all-features --quiet -- -D warnings`
  - `tests/run-applet-tests.sh`

## P0: Boot, Storage, and Recovery Closure

- `mount`
  - Expand helper/orchestration behavior where BusyBox-compatible userspace fallback is still narrow.
- `fsck`
  - Finish the remaining control-path gaps like `-T` and review pass scheduling behavior.
- `blkid`
  - Expand filesystem probe coverage toward the installer/storage matrix actually needed by the distro.
- `fdisk`
  - Decide whether DOS extended/logical partitions are required for installer/recovery workflows.
- `init`
  - Keep PID 1 hardening coverage strong; treat integration behavior as release-gated.

Status:
- In progress
- `mount`, `fsck`, `blkid`, and `init` have already had their first hardening pass, but are not yet considered closed.

## P0: Networking and Module Stack Closure

- `ip` / `ifconfig` / `netstat` / `ping`
  - Decide the intended BusyBox parity surface and close the remaining IPv6/option gaps that matter for real admin workflows.
- `udhcpc` / `udhcpd`
  - Finish client/server behaviors needed for real DHCP deployments, especially client-side ARP validation.
- `modprobe`
  - Close config parsing edge cases and test module policy behavior more aggressively.
- `ntpd`
  - Decide whether the current small daemon is sufficient or whether clock discipline needs another step up.

Status:
- Not started as a dedicated stabilization pass

## P0: Differential and Edge-Case Coverage

- `awk`
  - Differential test against BusyBox for the currently supported language surface.
  - Close or explicitly document the remaining unsupported forms.
- `sed`
  - Differential test nested blocks, range behavior, and NUL/regex edges more aggressively.
- Archive/compression applets
  - Add malformed/truncated and partial-write coverage where the current suite is still light.

Status:
- Not started as a dedicated stabilization pass

## P1: Session and Account Stack

- `getty`
  - Decide how much real tty line discipline is required.
- `login` / `su` / `sulogin`
  - Decide whether the distro target needs PAM or whether the current non-PAM model is intentional.
  - If PAM stays out of scope, tighten and document the non-PAM support contract clearly.
- `shutdown` / `halt` / `poweroff` / `reboot`
  - Decide whether `-w` must grow real utmp/wtmp behavior or remain intentionally unsupported.

Status:
- Not started as a dedicated stabilization pass

## P1: Architectural Cleanup

- Split large mixed applets into clearer layers where that materially lowers regression risk.
  - `awk`: parse / eval / runtime / builtins
  - `sed`: parse / execution / regex-text helpers / file I/O policy
- Separate live Linux backends from state-file test backends in shared helpers like `common/net`.
- Keep the cleanup narrow and behavior-preserving; this is a hardening pass, not a redesign.

Status:
- Not started

## P1: Error Model and Diagnostics

- Preserve BusyBox-style stderr output for users.
- Improve internal error classification enough to make risky codepaths easier to audit and test.
- Add more explicit failure-path tests for syscalls, partial writes, unreadable inputs, and malformed config.

Status:
- Not started

## Support Matrix

- Parity targets
  - Core shell/text applets used directly in scripts and bootstrap flows
  - Boot/storage/network/module applets used by installer and base system
- Intentionally smaller applets
  - Applets where BusyBox has a much broader surface but the distro only needs a narrow, documented subset
- Out of scope
  - Features explicitly rejected in `APPLETS.md` or here after review

Any Tier 1 applet that remains intentionally smaller should have that called out in `APPLETS.md`.

## First Gate

The initial stabilization gate should be runnable with:

```sh
bin/alpine-stabilization
```

That gate is expected to cover:

- unprivileged Alpine `cargo test --quiet`
- unprivileged Alpine `cargo clippy --all-targets --all-features --quiet -- -D warnings`
- unprivileged Alpine `tests/run-applet-tests.sh`
- privileged Linux `tests/run-linux-init.sh`
- privileged Linux `tests/run-linux-phase7b.sh --privileged`

## Immediate Next Steps

1. Run the new stabilization gate and fix any red phases.
2. Do a dedicated P0 networking closure pass.
3. Start differential hardening for `awk` and `sed`.
4. Revisit session/login scope and decide what is intentionally non-goal versus unfinished.

## Progress Log

- 2026-04-14
  - added the initial Tier 1 stabilization gate: `tests/run-tier1-stabilization.sh` and `bin/alpine-stabilization`
  - fixed a false-red `man` unit test by decoupling it from `/usr/bin/man` existing in Alpine
  - fixed `tests/run-applet-tests.sh` to honor `CARGO_TARGET_DIR`, so Alpine shell tests use the Linux binary instead of the host build
  - fixed `sed` so undefined branch labels are rejected at parse time, matching the shell regression case on empty input
  - fixed `patch` unit tests to avoid process-wide `cwd` mutation under parallel test execution
  - fixed multiple process-tool exit-status and matching bugs in `pgrep`, `pkill`, `pidof`, and `killall`
  - fixed `printf` missing-argument handling so `xargs` batch behavior now matches BusyBox for `%s`
  - fixed `dd` sparse-output and suffix parsing behavior, which unblocked `mkswap` and large-file fixture setup
  - hardened `tar` overwrite and hardlink handling, including preserved hardlinks across overwrite and hardlinked symlink entries
  - tightened `ls` block accounting and long-format rendering to match the Alpine `/bin/ls` oracle more closely
  - converted several shell tests to use explicit system-tool oracles where the previous failures were in the oracle applet rather than the applet under test
  - cleared the unprivileged Alpine quick gate end to end
