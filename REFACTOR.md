# Refactor Backlog

Keep this file short and current. Each item should be a discrete piece of work that can be finished with tests.

## Coverage

- [ ] Expand `tar` coverage further in the default runner.
- [ ] Add more malformed/truncated archive tests for `tar`.
- [ ] Add more compression edge-case tests beyond the current truncated-decode coverage.
- [ ] Add stronger `wget` coverage for redirects and HTTPS behavior, with clean offline skips.
- [ ] Add runner-level Linux fixture coverage for `mknod`.
- [ ] Add runner-level Linux fixture coverage for `losetup`.
- [ ] Add more failure-path tests for partial writes, unreadable files, and broken symlinks.

## Compatibility

- [ ] Keep tightening exact `ls` long-format compatibility where host comparisons still expose gaps.
- [ ] Audit stderr wording and exit codes applet by applet.
- [ ] Tighten exact formatting for `tar` listings and `date` output where tests expose drift.
- [ ] Document intentional host-specific behavior instead of leaving it implicit.

## Structure

- [ ] Review `grep` for simplification without changing behavior.
- [ ] Review `sort` for simplification without changing behavior.
- [ ] Review `find` for simplification without changing behavior.
- [ ] Review `tar` internals again after coverage settles.
- [ ] Consolidate shared test helpers where duplication is still obvious.

## Linux

- [ ] Extend the Alpine Phase 7b harness beyond smoke coverage.
- [ ] Add a glibc-based Linux target alongside Alpine/musl.
- [ ] Run Linux-only coverage under both privileged and unprivileged container modes where applicable.
- [ ] Write down which applets are expected to work on macOS, Linux, or both.

## Size And Perf

- [ ] Re-measure release binary size after the next major feature batch.
- [ ] Re-check `wget` TLS size if networking code changes again.
- [ ] Identify obvious hot-path allocation/syscall waste in `grep`, `sort`, `tar`, and `ls`.

## Next

- [ ] Continue with `tar` coverage and malformed-input hardening.
- [ ] Then strengthen `wget` network coverage.
- [ ] Then wire real Linux fixture coverage for `mknod` and `losetup`.
