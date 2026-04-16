# AGENTS

## Purpose

`seed` is a multi-call binary in the BusyBox style. Each applet is a small Rust module registered in `src/lib.rs`, and the binary dispatches based on `argv[0]` or the first argument after `seed`/`busybox`. Compatibility for `busybox <applet>` invocation matters in addition to direct `argv[0]` dispatch and `seed <applet>`.

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
- Prefer streaming over collecting whole inputs when the algorithm allows it.
- Minimize allocations and buffering unless the problem genuinely requires materializing data.
- Prefer the simplest correct implementation over abstraction-heavy designs.
- Match BusyBox behavior where that is the contract, but do not mechanically translate BusyBox C into Rust.

## Working Style

- Read the target applet, nearby applets, and the shared helpers before editing.
- Preserve existing conventions over clever refactors.
- Keep changes reviewable: one applet, one logical behavior change at a time.
- Do not widen public/shared interfaces unless another caller needs them now.
- If a helper would only have one caller, keep it local.

## Linux Dev Environment

- The target runtime is Linux on Alpine/musl.
- Treat the Alpine container as authoritative for Linux-only applets and behavior. Do not treat host-native results as the final oracle.
- Container files:
  - `Dockerfile.alpine-dev`: Alpine + Rust toolchain + test dependencies.
  - `Dockerfile.alpine-release`: Alpine release-build image for local CI-style packaging checks.
  - `docker-compose.yml`: main `dev` service and a `dev-privileged` variant for kernel/capability-heavy applets.
  - `bin/alpine-shell`: open an interactive shell in the normal Alpine dev container.
  - `bin/alpine-shell-privileged`: open an interactive shell in the privileged Alpine container.
  - `bin/alpine-cargo`: run `cargo ...` inside the normal Alpine container.
  - `bin/alpine-test`: run `tests/run-applet-tests.sh` in Alpine, or `cargo test --quiet ...` if given test filters.
  - `bin/alpine-clippy`: run the repo clippy command in Alpine.
  - `bin/alpine-release-build`: build and package the multicall release artifact in Alpine, including `bin/list-release-applets` and `bin/prepare-release-artifact`.
- Cargo caches live in Docker volumes, and Linux build artifacts live in the `cargo-target` volume rather than the host `target/` tree.
- Use `dev-privileged` only when an applet genuinely needs extra kernel access. Keep the normal loop on unprivileged `dev`.
- For local release repros, prefer `bin/alpine-release-build --debug` first, then `bin/alpine-release-build --release <x86_64|aarch64>` when validating the full release path.
- Use `bin/alpine-release-build --debug x86_64` to reproduce the GitHub x86 Alpine/musl build locally without waiting on CI.

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
- The BusyBox-style shell suite has two formats:
  - new-style `.tests` files via `testing.sh`
  - old-style standalone shell scripts where exit 0 means pass
- When behavior depends on Unix metadata, test the visible contract instead of overfitting to one implementation detail.
- For Linux-targeted work, prefer the Alpine wrappers over host execution:
  - `bin/alpine-test`
  - `bin/alpine-cargo test --quiet <filter>`
  - `bin/alpine-clippy`
- Preferred adaptation pattern for external test ideas:
  1. Lift the behavior being tested, not the exact script text.
  2. Rewrite the case in this repo's small shell-test style.
  3. Use the host tool as the oracle when that behavior is stable on the current platform.
  4. Pin flags explicitly when host defaults differ from BusyBox or `seed` defaults.
  5. Keep the test scoped to the applet or option being added; avoid dragging in unrelated semantics.
- Rewrite applicable existing tests when working on an applet if an adapted case is clearer, stronger, or less platform-fragile than what is already in the repo.
- Prefer incremental replacement by applet or test area so the suite stays reviewable and regressions stay attributable.
- Handle SIGPIPE quietly and keep failure paths non-panicking in normal applet execution.

## Commit Discipline

- One applet per commit.
- Keep doc-only or mechanical follow-ups separate when practical.
- Never revert unrelated local changes.
