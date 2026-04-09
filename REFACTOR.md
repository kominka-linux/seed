# Refactor Backlog

This file tracks the gap between the current `seed` codebase and a more complete, production-grade BusyBox-style implementation.

The goal is to keep each item discrete and shippable. Prefer finishing one item fully, with tests, before starting the next.

## 1. Test Coverage Expansion

- [ ] Wire more BusyBox fixture coverage into the default runner applet by applet.
- [ ] Expand `tar` fixture coverage beyond the currently enabled scripts.
- [x] Expand `ls` fixture coverage beyond the currently enabled scripts.
- [x] Promote a safe subset of `tests/busybox/ls.tests` into the default runner.
- [x] Add explicit regression coverage for `ls` symlink-to-directory behavior and multi-target formatting.
- [x] Decide which `ls` cases should compare against host `ls` and which should use fixed expected output.
- [ ] Keep tightening exact `ls` long-format compatibility where host comparisons expose gaps.
- [ ] Add more `date` fixture coverage if new cases appear; the current old-style fixture set is already wired into the runner.
- [ ] Add runner-level fixture coverage for `mknod` on Linux.
- [ ] Add runner-level fixture coverage for `losetup` on Linux.
- [ ] Add fixture coverage for compression edge cases across `gzip`, `bzip2`, `xz`, and `lzma`.
- [x] Add malformed/truncated decode coverage for `gzip`, `bzip2`, `xz`, and `lzma`.
- [ ] Add network-backed `wget` coverage for redirects, HTTPS success, and output-path handling with clean skip semantics when offline.
- [ ] Add failure-path tests for partial writes, unreadable files, permission errors, and broken symlinks.
- [ ] Add malformed-input tests for archive and compression applets.
- [ ] Add large-file tests for file-manipulation and archive applets.
- [ ] Add timezone- and locale-sensitive tests where output formatting depends on libc behavior.

## 2. Linux And Platform Matrix

- [ ] Promote the Alpine Phase 7b harness into a broader Linux verification harness.
- [ ] Add a second Linux target with glibc to complement Alpine/musl coverage.
- [ ] Run `mknod` and `losetup` coverage under both privileged and unprivileged container modes.
- [ ] Add targeted filesystem tests across tmpfs/ext4-like assumptions where behavior may diverge.
- [ ] Document which applets are expected to work on macOS, Linux, or both.
- [ ] Add explicit skip semantics for tests that require capabilities, devices, or outbound networking.

## 3. Argument Parsing Consistency

- [x] Audit all applets for inconsistent option parsing and error wording.
- [x] Extract a small shared parsing helper for common patterns only:
  - required option arguments
  - mutually exclusive modes
  - repeated-option diagnostics
  - `--` handling
- [x] Move one simple applet onto the helper first and verify it improves readability.
- [x] Move the remaining small applets incrementally rather than attempting a single large rewrite.
- [x] Keep applet-specific parsing local when abstraction would obscure behavior.

## 4. Error Semantics And Output Fidelity

- [ ] Audit stderr wording for all applets against BusyBox or host-tool expectations.
- [ ] Standardize exit-code behavior for success, usage errors, missing files, and partial failures.
- [ ] Audit newline behavior and stdout/stderr separation across all applets.
- [ ] Tighten exact formatting for long listings, archive listings, and date/time output.
- [ ] Add regression tests for all behavior fixes in this area.

## 5. Shared Filesystem And Metadata Helpers

- [ ] Review overlap between `ls`, `tar`, and file-copy/move applets for Unix metadata formatting and lookup.
- [ ] Extract only the stable shared pieces into `src/common`.
- [ ] Consolidate repeated tempdir/test helper logic into shared test utilities.
- [ ] Avoid pulling applet-specific policy into shared helpers.

## 6. Archive And Compression Hardening

- [ ] Audit `tar` behavior against a broader set of BusyBox and GNU tar cases.
- [ ] Add more coverage for hardlinks, sparse-ish archives, strange header sizes, and path-prefix corner cases.
- [ ] Improve compression autodetection and archive/list/extract parity where coverage exposes gaps.
- [ ] Audit path traversal and unsafe extraction behavior.
- [ ] Stress-test streaming paths with stdin/stdout pipelines and truncated input.
- [ ] Review the shared compression abstraction for unnecessary duplication or awkward call shapes.

## 7. Applet-Specific Cleanup

- [ ] Review `grep` for simplification opportunities without losing current behavior.
- [ ] Review `sort` for simplification opportunities without losing current behavior.
- [ ] Review `find` for simplification opportunities without losing current behavior.
- [ ] Review `diff` for simplification opportunities without losing current behavior.
- [ ] Review `tar` for internal simplification after compatibility work settles.
- [ ] Review the newer Linux-only applets (`mknod`, `losetup`) for API and error cleanup once coverage is broader.

## 8. Dependency And Binary Size Work

- [ ] Measure release binary size after each major feature area instead of waiting for the end.
- [ ] Audit dependency feature flags for every third-party crate.
- [ ] Prefer system trust or platform-verifier paths over bundled trust stores where practical.
- [ ] Re-check `wget` TLS size after future networking changes.
- [ ] Identify high-cost crates and measure whether the cost is code, const data, or debug/link metadata.
- [ ] Delay aggressive size hacks until behavior and coverage are more stable.

## 9. Performance Work

- [ ] Add basic timing benchmarks for representative applets:
  - `grep`
  - `sort`
  - `tar`
  - `ls`
  - compression applets
- [ ] Identify obviously avoidable allocations and copies in hot paths.
- [ ] Review directory traversal and metadata-fetch patterns for redundant syscalls.
- [ ] Optimize only after measuring.

## 10. Documentation And Project Hygiene

- [ ] Write a short support matrix for implemented applets and major flags.
- [ ] Document current intentional incompatibilities and missing surface area.
- [ ] Keep notes on platform-specific behavior that is accepted rather than fixed.
- [ ] Add short maintenance notes where tests require network, privileges, or special devices.
- [ ] Keep this file updated as items are completed or split.

## 11. Suggested Order

- [ ] Expand test coverage for the highest-risk applets first: `tar`, `ls`, `date`, `wget`, `mknod`, `losetup`.
- [ ] For the next non-Linux pass, prioritize `ls` coverage expansion before further structural refactors.
- [ ] Fix the newly exposed behavior gaps before starting deeper structural refactors.
- [ ] Introduce small shared parsing/helpers only after concrete duplication becomes painful.
- [ ] Revisit binary size and perf once compatibility coverage is substantially broader.
- [ ] Treat each completed refactor as incomplete until tests and fixture coverage move with it.
