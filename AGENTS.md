# AGENTS

## Purpose

`seed` is a multi-call binary in the BusyBox style. Each applet is a small Rust module registered in `src/lib.rs`, and the binary dispatches based on `argv[0]` or the first argument after `seed`/`busybox`.

## Codebase Structure

- `src/main.rs`: process entrypoint, signal setup, and dispatch into the library.
- `src/lib.rs`: central applet registry and dispatch logic. New applets are not live until registered here.
- `src/applets/`: one module per applet. Keep parsing, core behavior, and syscall wrappers local unless multiple applets genuinely share the logic.
- `src/common/`: shared building blocks used across applets.
  - `args.rs`: lightweight flag/operand parsing via `ArgCursor`.
  - `applet.rs`: `AppletResult` and `finish()` for consistent exit handling.
  - `error.rs`: user-facing applet errors.
  - `fs.rs`, `io.rs`, `runtime.rs`, `unix.rs`: reusable filesystem, I/O, runtime, and Unix helpers.
- `tests/busybox/`: shell-driven compatibility tests. Prefer adding focused applet tests here for user-visible behavior.
- `tests/run-applet-tests.sh`: builds the binary, creates applet symlinks, and runs the BusyBox-style test corpus.
- `tests/linux/`: broader Linux smoke/integration coverage for later phases.
- `APPLETS-TODO.md`: applet inventory and tiering. Tier 1 is the active backlog unless told otherwise.

## Applet Design Rules

- Keep applets small and explicit. Most applets should expose `pub fn main(args: &[String]) -> i32` and keep the real work in a `run(...)` helper returning `AppletResult` or `Result<_, Vec<AppletError>>`.
- Reuse existing helpers before adding new ones, but only extract shared code that is already shared in practice. Tight interfaces matter more than maximal deduplication.
- Prefer narrow data types over loose bags of state. If an applet has real structure, introduce a small `Options` type with only the fields it needs.
- Put syscall or platform-specific code behind small helpers so parsing and behavior remain easy to test.
- Follow existing error style: concise BusyBox-like messages through `AppletError`, with context about the failed action.
- Avoid adding dependencies unless there is a strong reason. The repo favors standard library and libc-first solutions.

## Working Style

- Read the target applet, nearby applets, and the shared helpers before editing.
- Preserve existing conventions over clever refactors.
- Keep changes reviewable: one applet, one logical behavior change at a time.
- Do not widen public/shared interfaces unless another caller needs them now.
- If a helper would only have one caller, keep it local.

## TDD Workflow

1. Add or extend a focused test first.
2. Run the smallest failing check you can.
3. Implement the minimum code to make that test pass.
4. Run incremental verification:
   `cargo test <unit-test>`
   `tests/run-applet-tests.sh` or a targeted shell test for the applet
   broader suites only after targeted checks pass

## Adding A New Applet

1. Create `src/applets/<name>.rs`.
2. Export the module from `src/applets/mod.rs`.
3. Register the applet in `src/lib.rs`.
4. Add focused tests:
   unit tests for parsing/internal helpers when useful
   BusyBox-style shell tests under `tests/busybox/<name>/` for observable behavior
5. Update `tests/run-applet-tests.sh` so the symlink farm includes the new applet and its tests run.
6. Mark the applet done in `APPLETS-TODO.md` once the implementation lands.

## Testing Notes

- Prefer targeted `cargo test` runs while developing.
- Keep shell tests small and direct. Match the existing style in `tests/busybox/*`.
- When behavior depends on Unix metadata, test the visible contract instead of overfitting to one implementation detail.

## Commit Discipline

- One applet per commit.
- Keep doc-only or mechanical follow-ups separate when practical.
- Never revert unrelated local changes.
