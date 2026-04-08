#!/bin/sh

set -eu

cd /src

cargo build --quiet

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-phase7b.XXXXXX")
trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

mknod "$tmpdir/fifo" p
test -p "$tmpdir/fifo"
printf 'phase7b: mknod-capable\n'

if [ "${SEED_REQUIRE_LOOP:-0}" = "1" ]; then
	test -c /dev/loop-control
	losetup -f >/dev/null
	printf 'phase7b: loop-capable\n'
elif [ -c /dev/loop-control ] && losetup -f >/dev/null 2>&1; then
	printf 'phase7b: loop-capable\n'
else
	printf 'phase7b: loop-unavailable (use --privileged for losetup)\n'
fi
