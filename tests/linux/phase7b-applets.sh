#!/bin/sh

set -eu

cd /src

cargo build --quiet

binary="${CARGO_TARGET_DIR:-target}/debug/seed"
links_dir=${TMPDIR:-/tmp}/seed-phase7b-links
tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-phase7b.XXXXXX")
system_losetup=$(command -v losetup)

trap 'rm -rf "$tmpdir" "$links_dir"' EXIT HUP INT TERM

mkdir -p "$links_dir"
for applet in mknod losetup; do
	ln -sf "$binary" "$links_dir/$applet"
done

PATH="$links_dir:$PATH"

mknod "$tmpdir/fifo" p
test -p "$tmpdir/fifo"

mknod -m 600 "$tmpdir/null" c 1 3
test -c "$tmpdir/null"
test x"`stat -c '%a' "$tmpdir/null"`" = x600

printf 'phase7b: mknod-ok\n'

if [ ! -c /dev/loop-control ]; then
	printf 'phase7b: losetup-skip (loop-control unavailable)\n'
	exit 0
fi

dd if=/dev/zero of="$tmpdir/disk.img" bs=1k count=64 status=none
loopdev=`losetup -f`
losetup "$loopdev" "$tmpdir/disk.img"
losetup -a | grep -F "$tmpdir/disk.img" >/dev/null
"$system_losetup" -a | grep -F "$tmpdir/disk.img" >/dev/null
losetup -c "$loopdev"
losetup -d "$loopdev"
"$system_losetup" -a | grep -F "$tmpdir/disk.img" >/dev/null && exit 1 || true

printf 'phase7b: losetup-ok\n'
