# TODO

Keep this short and current. Prefer discrete items with tests.

## Recently Closed

- [x] High-app review: add `find` support for `!`, `-a`, `-o`, and parenthesized expressions.
- [x] High-app review: preserve empty inline patterns and blank pattern-file lines in `grep`.
- [x] High-app review: make `timeout` terminate the whole process group and escalate from `SIGTERM` to `SIGKILL`.
- [x] High-app review: make `readlink -f` normalize `..` across canceled missing components.
- [x] High-app review: accept `+HH:MM` and `-HH:MM` timezone offsets in `date -d`.
- [x] Routine-app review: narrow `pgrep` / `pkill` / `killall` matching to the process name and `argv[0]`.
- [x] Routine-app review: make `watch` execute command strings through `sh -c`.
- [x] Routine-app review: make `hostname -s` real and reject extra operands.

## Coverage

- [ ] Expand `tar` coverage further in the default runner.
- [ ] Add more malformed and truncated archive tests for `tar`.
- [ ] Add more compression edge-case tests beyond the current truncated-decode coverage.
- [ ] Add stronger `wget` coverage for redirects and HTTPS behavior, with clean offline skips.
- [ ] Add runner-level Linux fixture coverage for `mknod`.
- [ ] Add runner-level Linux fixture coverage for `losetup`.
- [ ] Add more failure-path tests for partial writes, unreadable files, and broken symlinks.
- [ ] Add shell regression coverage for `sort`.
- [ ] Add shell regression coverage for `nohup`.

## Compatibility

- [ ] Keep tightening exact `ls` long-format compatibility where host comparisons still expose gaps.
- [ ] Audit stderr wording and exit codes applet by applet.
- [ ] Tighten exact formatting for `tar` listings and `date` output where tests expose drift.
- [ ] Document intentional host-specific behavior instead of leaving it implicit.

## Structure

- [ ] Review `grep` for simplification without changing behavior.
- [ ] Review `sort` for simplification without changing behavior.
- [ ] Review `find` for simplification without changing behavior after the boolean-expression rewrite settles.
- [ ] Review `tar` internals again after coverage settles.
- [ ] Consolidate shared test helpers where duplication is still obvious.

## Linux

- [ ] Extend the Alpine harness beyond smoke coverage.
- [ ] Run Linux-only coverage under both privileged and unprivileged container modes where applicable.
- [ ] Write down which applets are expected to work on Linux and which are intentionally unsupported.

## Size And Perf

- [ ] Re-measure release binary size after the next major feature batch.
- [ ] Re-check `wget` TLS size if networking code changes again.
- [ ] Identify obvious hot-path allocation or syscall waste in `grep`, `sort`, `tar`, and `ls`.

## Next

- [ ] Continue with `tar` coverage and malformed-input hardening.
- [ ] Then strengthen `wget` network coverage.
- [ ] Then wire real Linux fixture coverage for `mknod` and `losetup`.
