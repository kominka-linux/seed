Current focus: polish Phase 4 (`find` + `ls`) after finishing the tar/compression slice.

Completed since the last handoff:
- Added a shared compression workflow in [src/applets/compression.rs](/Users/josh/d/seed/src/applets/compression.rs).
- Refactored [src/applets/gzip.rs](/Users/josh/d/seed/src/applets/gzip.rs), [src/applets/bzip2.rs](/Users/josh/d/seed/src/applets/bzip2.rs), and [src/applets/xz.rs](/Users/josh/d/seed/src/applets/xz.rs) onto that helper.
- Extended tar fixture coverage in [tests/run-applet-tests.sh](/Users/josh/d/seed/tests/run-applet-tests.sh).
- Fixed `tar -tvf` compatibility in [src/applets/tar.rs](/Users/josh/d/seed/src/applets/tar.rs) by using a small raw-header walker for list mode while keeping `tar-rs` for create/extract.

Current verification status:
- `./tests/run-applet-tests.sh` passes.
- `make test` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.

Phase 4 status:
- The current baseline for `find` and `ls` is green against the bundled local runner.
- `ls` already matches the exercised old-style `-1`, `-h`, `-l`, and `-s` behavior, plus symlink-to-directory handling and correct suppression of `total` for direct file operands.
- The remaining work is polish and coverage expansion, not a known red test in the current suite.

Next target:
- Tighten `ls` multi-target behavior to match system `ls` more closely:
  - group non-directory operands before directory listings
  - print directory headers when more than one target is listed
  - insert blank lines between headered directory sections
- Add focused tests for that behavior, then rerun targeted and full verification.
