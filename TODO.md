# TODO

## Coverage

- Expand `tar` coverage further in the default runner.
- Add more malformed and truncated archive tests for `tar`.
- Add more compression edge-case tests beyond the current truncated-decode coverage.
- Add stronger `wget` coverage for redirects and HTTPS behavior, with clean offline skips.
- Add runner-level Linux fixture coverage for `mknod`.
- Add runner-level Linux fixture coverage for `losetup`.
- Add more failure-path tests for partial writes, unreadable files, and broken symlinks.
- Add shell regression coverage for `sort`.
- Add shell regression coverage for `nohup`.

## Compatibility

- Keep tightening exact `ls` long-format compatibility where host comparisons still expose gaps.
- Audit stderr wording and exit codes applet by applet.
- Tighten exact formatting for `tar` listings and `date` output where tests expose drift.
- Document intentional host-specific behavior instead of leaving it implicit.

## Structure

- Review `grep` for simplification without changing behavior.
- Review `sort` for simplification without changing behavior.
- Review `find` for simplification without changing behavior after the boolean-expression rewrite settles.
- Review `tar` internals again after coverage settles.
- Consolidate shared test helpers where duplication is still obvious.

## Linux Harness

- Extend the Alpine harness beyond smoke coverage.
- Run Linux-only coverage under both privileged and unprivileged container modes where applicable.
- Keep documenting which applets are expected to work on Linux and which are intentionally unsupported.
- Keep treating Alpine as the Linux oracle for Linux-only applets.

