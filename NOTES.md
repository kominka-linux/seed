# Notes

Implementation notes and handoff context for future agents.

## Current State

- The repo currently implements:
  - `cat`
  - `chmod`
  - `cp`
  - `diff`
  - `find`
  - `grep`
  - `ls`
  - `mkdir`
  - `mv`
  - `od`
  - `printf`
  - `rm`
  - `rmdir`
  - `sort`
  - `tee`
  - `wc`
- The current quality floor is:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `make test`
  - `make pc`
- The multi-call binary supports both `argv[0]` dispatch and `seed <applet>`.

## Shared Modules

- `src/common/fs.rs`
  - Holds shared copy/move/remove logic.
  - `cp` and `mv` depend on it.
  - Uses typed `CopyError` instead of stringly sentinel errors.
- `src/common/io.rs`
  - Holds stream-copy and stdin/stdout/file helpers.
  - The earlier leaked `'static` stdio handles were removed.
- `src/common/runtime.rs`
  - Holds argv collection and SIGPIPE setup.
  - Now uses vendored `libc` instead of hand-written FFI declarations.
- `src/common/applet.rs`
  - Small shared applet result plumbing.

## Dependency Note

- `libc` is vendored at `vendor/libc` and used as a path dependency.
- Reason:
  - we already needed Unix FFI
  - `libc` is safer than maintaining hand-written ABI declarations and magic constants
  - the environment used by agents may not have network access, so vendoring avoids crates.io resolution failures

## Test Runner

- `tests/run-applet-tests.sh` is the canonical local BusyBox runner.
- It builds the binary, creates temporary applet symlinks, and runs both:
  - new-style `.tests`
  - old-style shell tests
- The script also provides a small `echo-ne` shim for this macOS environment.
- `find.tests` is now enabled with the bundled `FEATURE_FIND_*` buckets.
- `ls.tests` is enabled with `CONFIG_FEATURE_LS_SORTFILES=y`.
- The old-style `ls` scripts run against a controlled temporary fixture so
  they compare our `ls` output to the system `ls` output on stable input.
- Those old-style `ls` scripts rely on the system `diff`, not our applet,
  because they use `diff -ubw`.

## Important Gaps

- Phase 3 is close to complete against the currently useful local surface.
- The only intentionally deferred BusyBox bucket in the current runner is:
  - `od`: non-desktop `-e` / `-F`
- Stage 4 has started, but `ls` is still a minimal compatibility
  implementation aimed at the exercised tests rather than the entire BusyBox
  option surface.

## Architecture Assessment

- The codebase is in decent shape for continued work, but it is not yet fully disciplined.
- Good:
  - shared filesystem/platform code is starting to consolidate properly
  - Phase 3 allocation debt was reduced by making `grep` line-streaming and `od` block-streaming
  - verification discipline is good
- Still weak:
  - applet entrypoint and error-handling style is inconsistent across applets
  - some option parsing still relies on boolean clusters where enums/typed structs would be better
  - `ls` is intentionally applet-local at the moment; if more of its surface
    is implemented, shared Unix formatting/path helpers may become worth
    extracting

## Recommended Next Steps

1. Normalize applet shape:
   - prefer one consistent pattern like `parse_args -> typed options -> run -> finish`
   - unify on shared error plumbing instead of mixing `AppletResult` and handwritten `Result<i32, String>`
2. When it matters, finish the deferred `od` non-desktop bucket and remove
   the last intentional skip from `tests/run-applet-tests.sh`.
3. Deepen Phase 4 deliberately:
   - extend `find` only if more predicate/action surface is actually needed
   - extend `ls` from the current tested baseline instead of jumping straight
     to the full BusyBox flag matrix
4. Keep shared logic shared:
   - if `find`, `ls`, `tar`, or later applets need more filesystem traversal,
     path handling, or Unix formatting logic, extend `common` rather than
     duplicating applet-local code

## Practical Warnings

- Do not assume Phase 3 is done just because `make test` is green. It is green for the currently enabled test surface.
- The repo may be worked on in a sandbox where `.git` writes are blocked. In that environment, code edits and verification can succeed while `git commit` fails.
