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
for applet in ifconfig ip losetup mknod; do
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

if [ ! -f /proc/net/if_inet6 ] || ! grep -q ' lo$' /proc/net/if_inet6; then
	printf 'phase7b: ipv6-skip (loopback IPv6 unavailable)\n'
	exit 0
fi

ip -6 addr add 2001:db8:7::2/128 dev lo
ip -6 addr show dev lo | grep -F 'inet6 2001:db8:7::2/128' >/dev/null
ip -6 addr del 2001:db8:7::2/128 dev lo
ip -6 addr show dev lo | grep -F 'inet6 2001:db8:7::2/128' >/dev/null && exit 1 || true

printf 'phase7b: ip-ipv6-addr-ok\n'

ip addr add 127.0.0.2/32 dev lo
ip addr show dev lo | grep -F 'inet 127.0.0.2/32' >/dev/null
ip addr del 127.0.0.2/32 dev lo
ip addr show dev lo | grep -F 'inet 127.0.0.2/32' >/dev/null && exit 1 || true

printf 'phase7b: ip-ipv4-addr-ok\n'

ifconfig lo add 2001:db8:7::3/128
ifconfig lo | grep -F 'inet6 addr: 2001:db8:7::3/128' >/dev/null
ifconfig lo del 2001:db8:7::3
ifconfig lo | grep -F 'inet6 addr: 2001:db8:7::3/128' >/dev/null && exit 1 || true

printf 'phase7b: ifconfig-ipv6-addr-ok\n'

ifconfig lo add 127.0.0.3/32
ifconfig lo | grep -F 'inet addr:127.0.0.3' >/dev/null
ifconfig lo del 127.0.0.3
ifconfig lo | grep -F 'inet addr:127.0.0.3' >/dev/null && exit 1 || true

printf 'phase7b: ifconfig-ipv4-addr-ok\n'

ip -6 route add 2001:db8:55::/64 dev lo
ip -6 route show | grep -F '2001:db8:55::/64 dev lo' >/dev/null
ip -6 route del 2001:db8:55::/64 dev lo
ip -6 route show | grep -F '2001:db8:55::/64 dev lo' >/dev/null && exit 1 || true

printf 'phase7b: ip-ipv6-route-ok\n'
