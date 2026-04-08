# Notes

Implementation notes and handoff context for future agents.

## Current State

- The repo currently implements:
  - `cat`
  - `chmod`
  - `cp`
  - `diff`
  - `grep`
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

## Important Gaps

- Phase 3 is not fully complete against `PLAN.md`.
- The current runner intentionally skips some BusyBox feature buckets:
  - `sort`: most of the larger option surface is still skipped
  - `grep`: `-E`/`egrep` and some NUL-handling coverage are still skipped
  - `diff`: recursive directory diff coverage is still skipped
  - `od`: some optional long-option and non-desktop cases are still skipped

## Architecture Assessment

- The codebase is in decent shape for continued work, but it is not yet fully disciplined.
- Good:
  - shared filesystem/platform code is starting to consolidate properly
  - Phase 3 allocation debt was reduced by making `grep` line-streaming and `od` block-streaming
  - verification discipline is good
- Still weak:
  - applet entrypoint and error-handling style is inconsistent across applets
  - some option parsing still relies on boolean clusters where enums/typed structs would be better
  - `sort` is especially underbuilt relative to the plan

## Recommended Next Steps

1. Normalize applet shape:
   - prefer one consistent pattern like `parse_args -> typed options -> run -> finish`
   - unify on shared error plumbing instead of mixing `AppletResult` and handwritten `Result<i32, String>`
2. Finish Phase 3 before starting Phase 4:
   - expand `sort`
   - expand `grep`
   - expand `diff`
   - then remove skips from `tests/run-applet-tests.sh`
3. Keep shared logic shared:
   - if `find`, `ls`, `tar`, or later applets need filesystem traversal or path handling, extend `common` rather than duplicating applet-local code

## Practical Warnings

- Do not assume Phase 3 is done just because `make test` is green. It is green for the currently enabled test surface.
- The repo may be worked on in a sandbox where `.git` writes are blocked. In that environment, code edits and verification can succeed while `git commit` fails.
