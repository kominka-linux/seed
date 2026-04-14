#!/bin/sh

set -eu

repo_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$repo_dir"

printf 'tier1: cargo test\n'
cargo test --quiet

printf 'tier1: cargo clippy\n'
cargo clippy --all-targets --all-features --quiet -- -D warnings

printf 'tier1: busybox shell suite\n'
sh tests/run-applet-tests.sh
