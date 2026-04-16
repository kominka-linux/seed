#!/bin/sh

set -eu

repo_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
target_dir=${CARGO_TARGET_DIR:-"$repo_dir/target"}
binary="$target_dir/debug/seed"
links_dir=${TMPDIR:-/tmp}/seed-links

make_echo_ne() {
	cat >"$1" <<'EOF'
#!/bin/sh
newline=1
interpret=0
while [ $# -gt 0 ]; do
	case "$1" in
		-ne|-en)
			newline=0
			interpret=1
			shift
			;;
		-n)
			newline=0
			shift
			;;
		-e)
			interpret=1
			shift
			;;
		--)
			shift
			break
			;;
		*)
			break
			;;
	esac
done

if [ "$interpret" -eq 1 ]; then
	printf '%b' "${1-}"
else
	printf '%s' "${1-}"
fi

if [ "$newline" -eq 1 ]; then
	printf '\n'
fi
EOF
	chmod +x "$1"
}

run_new_style() {
	test_name=$1
	optionflags=${2-}
	script_name=$(basename "$test_name")
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	printf 'new-style: %s\n' "$test_name"
	(
		ln -sf "$repo_dir/tests/busybox/testing.sh" "$tmpdir/testing.sh"
		ln -sf "$repo_dir/tests/busybox/$test_name" "$tmpdir/$script_name"
		cd "$tmpdir"
		PATH="$links_dir:$PATH" \
			ECHO="$echo_ne" \
			OPTIONFLAGS="$optionflags" \
			sh "./$script_name"
	)
	rm -rf "$tmpdir"
}

run_new_style_ls() {
	test_name=ls.tests
	script_name=$test_name
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	printf 'new-style: %s\n' "$test_name"
	(
		ln -sf "$repo_dir/tests/busybox/testing.sh" "$tmpdir/testing.sh"
		ln -sf "$repo_dir/tests/busybox/$test_name" "$tmpdir/$script_name"
		cd "$tmpdir"
		PATH="$links_dir:$PATH" \
			ECHO="$echo_ne" \
			CONFIG_FEATURE_LS_SORTFILES=y \
			sh "./$script_name"
	)
	rm -rf "$tmpdir"
}

run_old_style() {
	test_script=$1
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	printf 'old-style: %s\n' "$test_script"
	(
		cd "$tmpdir"
		PATH="$links_dir:$PATH" ECHO="$echo_ne" sh "$repo_dir/$test_script"
	)
	rm -rf "$tmpdir"
}

internet_tests_enabled() {
	if [ "${SKIP_INTERNET_TESTS:-}" != "" ]; then
		return 1
	fi
	if [ "${RUN_INTERNET_TESTS:-}" = "1" ]; then
		return 0
	fi
	if ! command -v python3 >/dev/null 2>&1; then
		return 1
	fi
	python3 - <<'PY'
import socket
import sys

try:
    socket.getaddrinfo("example.com", 443, type=socket.SOCK_STREAM)
    with socket.create_connection(("example.com", 443), timeout=3):
        pass
except OSError:
    sys.exit(1)
PY
}

run_old_style_internet() {
	test_script=$1
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	printf 'old-style: %s\n' "$test_script"
	skip=
	if ! internet_tests_enabled; then
		skip=1
	fi
	(
		cd "$tmpdir"
		PATH="$links_dir:$PATH" \
			ECHO="$echo_ne" \
			SKIP_INTERNET_TESTS="$skip" \
			WGET_TEST_URL="${WGET_TEST_URL:-https://example.com/}" \
			sh "$repo_dir/$test_script"
	)
	rm -rf "$tmpdir"
}

run_old_style_ls() {
	test_script=$1
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	fixture_dir="$tmpdir/ls-fixture"
	diff_dir=$(dirname "$(command -v diff)")
	printf 'old-style: %s\n' "$test_script"
	mkdir -p "$fixture_dir"
	printf 'a\n' >"$fixture_dir/a"
	printf '1234567890' >"$fixture_dir/big"
	(
		cd "$tmpdir"
		PATH="$diff_dir:$links_dir:$PATH" ECHO="$echo_ne" d="$fixture_dir" sh "$repo_dir/$test_script"
	)
	rm -rf "$tmpdir"
}

mkdir -p "$links_dir"
cargo build --quiet --manifest-path "$repo_dir/Cargo.toml"

is_linux=0
if [ "$(uname -s)" = "Linux" ]; then
	is_linux=1
fi

applets="addgroup adduser awk busybox bunzip2 bzip2 bzcat cat chattr chmod chgrp chown cmp cp cpio date dd delgroup deluser diff du egrep env expr find flock free fsck getopt getty grep gunzip gzip hexdump install killall less ln login ls lsattr lzcat lzma man mkdir mkfifo mkswap mv nologin nslookup ntpd od passwd paste patch pidof pgrep pkill printf ps pstree readlink realpath rm rmdir run-parts setsid sleep sort split stat stty su sulogin tar tee test time touch timeout tr tree udhcpc udhcpd uname unlzma unxz unzip watch wc wget xargs xz xzcat zcat [ [["
applets="$applets sed"
if [ "$is_linux" -eq 1 ]; then
	applets="$applets acpid blkid chroot depmod df dmesg fdisk halt hwclock ifconfig init insmod ip killall5 losetup lsmod lsof mdev mknod modinfo modprobe mount mountpoint netstat ping ping6 pivot_root poweroff reboot rfkill rmmod swapoff swapon switch_root sysctl umount uptime"
fi

for applet in $applets; do
	ln -sf "$binary" "$links_dir/$applet"
done

echo_ne=$(mktemp "${TMPDIR:-/tmp}/seed-echo-ne.XXXXXX")
trap 'rm -f "$echo_ne"' EXIT HUP INT TERM
make_echo_ne "$echo_ne"

run_new_style cat.tests ':FEATURE_CATV:FEATURE_CATN:'
run_new_style cp.tests
run_new_style diff.tests ':FEATURE_DIFF_DIR:'
run_new_style find.tests ':FEATURE_FIND_TYPE:FEATURE_FIND_EXEC:FEATURE_FIND_EXEC_PLUS:FEATURE_FIND_EXEC_OK:FEATURE_FIND_MAXDEPTH:'
run_new_style grep.tests ':EXTRA_COMPAT:EGREP:'
run_new_style_ls
run_new_style od.tests ':DESKTOP:LONG_OPTS:'
run_new_style printf.tests
run_new_style sort.tests ':FEATURE_SORT_BIG:'
run_new_style tar.tests ':FEATURE_SEAMLESS_GZ:GUNZIP:'

run_old_style tests/busybox/cat/cat-prints-a-file
run_old_style tests/busybox/cat/cat-prints-a-file-and-standard-input

run_old_style tests/busybox/addgroup/addgroup-creates-group
run_old_style tests/busybox/addgroup/addgroup-adds-user-to-existing-group
run_old_style tests/busybox/adduser/adduser-creates-user-group-shadow-and-home
run_old_style tests/busybox/adduser/adduser-supports-existing-primary-group
run_old_style tests/busybox/adduser/adduser-adds-existing-user-to-group
run_old_style tests/busybox/delgroup/delgroup-removes-user-from-group
run_old_style tests/busybox/delgroup/delgroup-deletes-unused-group
run_old_style tests/busybox/delgroup/delgroup-refuses-primary-group
run_old_style tests/busybox/deluser/deluser-removes-user-from-account-files
run_old_style tests/busybox/deluser/deluser-remove-home
run_old_style tests/busybox/passwd/passwd-deletes-password
run_old_style tests/busybox/passwd/passwd-locks-and-unlocks-password
run_old_style tests/busybox/passwd/passwd-sets-default-sha512-password
run_old_style tests/busybox/passwd/passwd-supports-md5-algorithm
run_old_style tests/busybox/passwd/passwd-rejects-mismatched-passwords
run_old_style tests/busybox/getty/getty-n-uses-custom-login-and-term
run_old_style tests/busybox/getty/getty-prompts-and-passes-username
run_old_style tests/busybox/login/login-force-uses-user-shell-and-home
run_old_style tests/busybox/login/login-p-preserves-home
run_old_style tests/busybox/awk/awk-field-separator-regex-counts-fields
run_old_style tests/busybox/awk/awk-begin-if-comparisons-work
run_old_style tests/busybox/awk/awk-input-is-never-octal
run_old_style tests/busybox/awk/awk-bitwise-or-and-hex-octal-constants-work
run_old_style tests/busybox/awk/awk-printf-floating-constants-work
run_old_style tests/busybox/awk/awk-long-field-separator-and-length-work
run_old_style tests/busybox/awk/awk-field-separator-handles-hex-escape
run_old_style tests/busybox/awk/awk-begin-sees-empty-record-fields
run_old_style tests/busybox/awk/awk-concatenation-is-not-a-function-call
run_old_style tests/busybox/awk/awk-v-sets-variables-before-begin
run_old_style tests/busybox/awk/awk-e-loads-program-snippet
run_old_style tests/busybox/awk/awk-f-loads-program-file
run_old_style tests/busybox/awk/awk-empty-function-noargs-works
run_old_style tests/busybox/awk/awk-function-return-value-works
run_old_style tests/busybox/awk/awk-undefined-function-errors
run_old_style tests/busybox/awk/awk-string-cast-and-ternary-work
run_old_style tests/busybox/awk/awk-whitespace-before-array-subscript-works
run_old_style tests/busybox/awk/awk-delete-array-postdecrement-evaluates-once
run_old_style tests/busybox/awk/awk-nested-for-in-loops-with-same-variable-work
run_old_style tests/busybox/awk/awk-large-integer-and-int-work
run_old_style tests/busybox/awk/awk-length-array-works
run_old_style tests/busybox/awk/awk-do-while-works
run_old_style tests/busybox/awk/awk-f-dash-and-argc-work
run_old_style tests/busybox/awk/awk-fs-assignment-works
run_old_style tests/busybox/awk/awk-getline-reads-current-input
run_old_style tests/busybox/awk/awk-getline-expression-reads-current-input
run_old_style tests/busybox/awk/awk-getline-expression-reads-file
run_old_style tests/busybox/awk/awk-getline-file-source-expression-works
run_old_style tests/busybox/awk/awk-nextfile-skips-to-next-input
run_old_style tests/busybox/awk/awk-delete-missing-arg-errors
run_old_style tests/busybox/awk/awk-top-level-colon-errors
run_old_style tests/busybox/awk/awk-negative-field-access-errors
run_old_style tests/busybox/awk/awk-break-else-parses
run_old_style tests/busybox/awk/awk-continue-else-parses
run_old_style tests/busybox/awk/awk-getline-nonexistent-file-sets-errno
run_old_style tests/busybox/awk/awk-gsub-falls-back-to-basic-regex
run_old_style tests/busybox/awk/awk-plus-assign-works
run_old_style tests/busybox/awk/awk-length-value-and-function-work
run_old_style tests/busybox/awk/awk-boolean-operators-work
run_old_style tests/busybox/awk/awk-regex-match-operators-work
run_old_style tests/busybox/awk/awk-regex-rule-pattern-works
run_old_style tests/busybox/awk/awk-while-loop-works
run_old_style tests/busybox/awk/awk-string-builtins-work
run_old_style tests/busybox/awk/awk-classic-for-loop-works
run_old_style tests/busybox/awk/awk-next-skips-rest-of-action
run_old_style tests/busybox/awk/awk-exit-status-works
run_old_style tests/busybox/awk/awk-sub-replacement-ampersand-works
run_old_style tests/busybox/awk/awk-split-regex-literal-works
run_old_style tests/busybox/sed/sed-substitutes-and-prints-lines
run_old_style tests/busybox/sed/sed-accepts-blanks-before-command
run_old_style tests/busybox/sed/sed-append-autoinserts-newline
run_old_style tests/busybox/sed/sed-append-autoinserts-newline-across-files
run_old_style tests/busybox/sed/sed-n-range-p-and-delete-work
run_old_style tests/busybox/sed/sed-E-backrefs-and-global-substitution-work
run_old_style tests/busybox/sed/sed-f-loads-script-file
run_old_style tests/busybox/sed/sed-long-expression-option-works
run_old_style tests/busybox/sed/sed-long-file-option-works
run_old_style tests/busybox/sed/sed-long-in-place-option-works
run_old_style tests/busybox/sed/sed-i-creates-backup-and-edits-in-place
run_old_style tests/busybox/sed/sed-in-place-address-modifies-all-files
run_old_style tests/busybox/sed/sed-in-place-finishes-ranges-correctly
run_old_style tests/busybox/sed/sed-insert-does-not-autoinsert-newline
run_old_style tests/busybox/sed/sed-print-autoinserts-newlines-two-files
run_old_style tests/busybox/sed/sed-selective-matches-insert-newline
run_old_style tests/busybox/sed/sed-zero-length-global-match-advances
run_old_style tests/busybox/sed/sed-zero-length-class-match-advances
run_old_style tests/busybox/sed/sed-global-substitute-does-not-double-advance
run_old_style tests/busybox/sed/sed-trailing-space-regex-global
run_old_style tests/busybox/sed/sed-branch-and-test-work
run_old_style tests/busybox/sed/sed-N-loop-flattens-multiline
run_old_style tests/busybox/sed/sed-hold-space-and-transliterate-work
run_old_style tests/busybox/sed/sed-n-reads-next-line-and-continues
run_old_style tests/busybox/sed/sed-empty-regex-reuses-last-pattern
run_old_style tests/busybox/sed/sed-empty-substitute-backref-uses-address-regex
run_old_style tests/busybox/sed/sed-address-matches-newline
run_old_style tests/busybox/sed/sed-c-command-replaces-lines
run_old_style tests/busybox/sed/sed-d-short-circuits-following-commands
run_old_style tests/busybox/sed/sed-caret-or-not-caret
run_old_style tests/busybox/sed/sed-understands-carriage-return-in-replacement
run_old_style tests/busybox/sed/sed-escaped-newline-in-command
run_old_style tests/busybox/sed/sed-nested-blocks-work
run_old_style tests/busybox/sed/sed-nested-block-range-addresses-work
run_old_style tests/busybox/sed/sed-N-and-P-work
run_old_style tests/busybox/sed/sed-text-commands-decode-escapes
run_old_style tests/busybox/sed/sed-i-command-decode-escapes
run_old_style tests/busybox/sed/sed-s-substitute-write-file
run_old_style tests/busybox/sed/sed-duplicate-write-file-target
run_old_style tests/busybox/sed/sed-special-delimiter-replacement-literal-ampersand
run_old_style tests/busybox/sed/sed-special-delimiter-replacement-literal-digit
run_old_style tests/busybox/sed/sed-special-left-bracket-replacement
run_old_style tests/busybox/sed/sed-special-delimiter-in-pattern
run_old_style tests/busybox/sed/sed-regex-line-range-addresses-work
run_old_style tests/busybox/sed/sed-regex-plus-range-works
run_old_style tests/busybox/sed/sed-regex-plus-zero-range-works
run_old_style tests/busybox/sed/sed-regex-plus-zero-range-in-place-two-files
run_old_style tests/busybox/sed/sed-r-reads-file-after-line
run_old_style tests/busybox/sed/sed-n-resets-substituted-bit
run_old_style tests/busybox/sed/sed-N-inside-block-keeps-running-block
run_old_style tests/busybox/sed/sed-d-does-not-break-line-range
run_old_style tests/busybox/sed/sed-d-does-not-break-regex-range
run_old_style tests/busybox/sed/sed-d-does-not-break-descending-line-range
run_old_style tests/busybox/sed/sed-backslash-dollar-multiline-regex-works
run_old_style tests/busybox/sed/sed-embedded-nul-substitution
run_old_style tests/busybox/sed/sed-embedded-nul-global-substitution
run_old_style tests/busybox/sed/sed-uses-previous-regexp
run_old_style tests/busybox/sed/sed-substitute-occurrence-then-global
run_old_style tests/busybox/sed/sed-beginning-anchor-matches-only-once
run_old_style tests/busybox/sed/sed-match-eof-without-trailing-newline
run_old_style tests/busybox/sed/sed-match-eof-two-files
run_old_style tests/busybox/sed/sed-multiple-e-text-commands-work
run_old_style tests/busybox/sed/sed-nonexistent-label-errors
run_old_style tests/busybox/lsattr/lsattr-v-lists-version-and-long-flags
run_old_style tests/busybox/lsattr/lsattr-R-a-recurses-hidden-entries
run_old_style tests/busybox/su/su-runs-command-as-target-user
run_old_style tests/busybox/su/su-login-mode-sets-home
run_old_style tests/busybox/su/su-preserve-env-keeps-home
run_old_style tests/busybox/sulogin/sulogin-force-runs-root-shell
run_old_style tests/busybox/sulogin/sulogin-eof-continues-boot
run_old_style tests/busybox/ntpd/ntpd-q-w-queries-explicit-peer
run_old_style tests/busybox/ntpd/ntpd-q-w-loads-peers-from-config
run_old_style tests/busybox/ntpd/ntpd-q-w-authenticates-explicit-keyed-peer
run_old_style tests/busybox/ntpd/ntpd-runs-script-after-sync
run_old_style tests/busybox/udhcpc/udhcpc-acquires-lease-and-runs-script
run_old_style tests/busybox/udhcpc/udhcpc-exports-bootfile-and-sname
run_old_style tests/busybox/udhcpc/udhcpc-r-requests-specific-address
run_old_style tests/busybox/udhcpc/udhcpc-R-releases-lease
run_old_style tests/busybox/udhcpc/udhcpc-a-declines-conflicting-lease
run_old_style tests/busybox/udhcpd/udhcpd-rejects-invalid-pool
run_old_style tests/busybox/udhcpd/udhcpd-respects-decline-and-reoffers-next-address

run_old_style tests/busybox/basename/basename-strips-suffix
run_old_style tests/busybox/basename/basename-multiple-names
run_old_style tests/busybox/basename/dirname-basic
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/depmod/depmod-generates-deps-and-aliases
	run_old_style tests/busybox/modinfo/modinfo-reads-module-fields
	run_old_style tests/busybox/modprobe/modprobe-loads-alias-and-dependencies
	run_old_style tests/busybox/modprobe/modprobe-r-unloads-reverse-order
	run_old_style tests/busybox/modprobe/modprobe-honors-softdep-options-and-install-command
	run_old_style tests/busybox/modprobe/modprobe-honors-remove-directive
	run_old_style tests/busybox/modprobe/modprobe-blacklist-blocks-alias-but-not-explicit-module
	run_old_style tests/busybox/modprobe/modprobe-install-command-keeps-hash-inside-quotes
	run_old_style tests/busybox/modprobe/modprobe-alias-options-apply-to-target
	run_old_style tests/busybox/acpid/acpid-runs-direct-handler-from-proc-events
	run_old_style tests/busybox/acpid/acpid-directory-handler-uses-run-parts
	run_old_style tests/busybox/init/init-runs-sysinit-and-respawns
	run_old_style tests/busybox/init/init-runs-shutdown-action-on-signal
	run_old_style tests/busybox/init/init-runs-restart-command-on-hup
	run_old_style tests/busybox/init/init-respawn-entry-can-bind-test-tty
run_old_style tests/busybox/blkid/blkid-lists-cache-backed-devices
run_old_style tests/busybox/blkid/blkid-detects-swap-header
run_old_style tests/busybox/blkid/blkid-detects-ext-superblock
run_old_style tests/busybox/blkid/blkid-detects-vfat-boot-sector
run_old_style tests/busybox/blkid/blkid-detects-iso9660-volume
run_old_style tests/busybox/blkid/blkid-detects-squashfs-superblock
run_old_style tests/busybox/fsck/fsck-T-suppresses-title
	run_old_style tests/busybox/fdisk/fdisk-l-prints-mbr-partition-table
	run_old_style tests/busybox/fdisk/fdisk-s-prints-image-size-in-kib
	run_old_style tests/busybox/fdisk/fdisk-script-creates-dos-partition
	run_old_style tests/busybox/fdisk/fdisk-script-creates-dos-extended-logical-partition
	run_old_style tests/busybox/fdisk/fdisk-script-creates-gpt-efi-partition
	run_old_style tests/busybox/mdev/mdev-s-creates-renamed-node-and-link
	run_old_style tests/busybox/mdev/mdev-hotplug-add-remove-runs-commands
	run_old_style tests/busybox/mdev/mdev-loads-firmware
fi

run_old_style tests/busybox/chattr/chattr-sets-and-clears-flags
run_old_style tests/busybox/chattr/chattr-R-sets-version-on-descendants
run_old_style tests/busybox/chmod/chmod-R-descends-before-changing-directory
run_old_style tests/busybox/chgrp/chgrp-sets-group
run_old_style tests/busybox/chown/chown-sets-owner
run_old_style tests/busybox/chown/chown-sets-owner-and-group
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/chroot/chroot-propagates-command-exit-status
	run_old_style tests/busybox/chroot/chroot-rejects-missing-root
	run_old_style tests/busybox/chroot/chroot-runs-command-in-new-root
	run_old_style tests/busybox/chroot/chroot-fails-missing-root-path
fi

run_old_style tests/busybox/cmp/cmp-identical-files
run_old_style tests/busybox/cmp/cmp-different-files-exit-one
run_old_style tests/busybox/cmp/cmp-reads-stdin
run_old_style tests/busybox/cmp/cmp-s-detects-difference
run_old_style tests/busybox/cpio/cpio-create-extract-roundtrip
run_old_style tests/busybox/cpio/cpio-lists-archive-members
run_old_style tests/busybox/cpio/cpio-passthrough-strips-leading-slash
run_old_style tests/busybox/cpio/cpio-create-requires-newc

run_old_style tests/busybox/cut/cut-selects-bytes
run_old_style tests/busybox/cut/cut-selects-fields
run_old_style tests/busybox/cut/cut-suppresses-undelimited-lines

run_old_style tests/busybox/date/date-@-works
run_old_style tests/busybox/date/date-D-works
run_old_style tests/busybox/date/date-I-works
run_old_style tests/busybox/date/date-R-works
run_old_style tests/busybox/date/date-format-works
run_old_style tests/busybox/date/date-timezone
run_old_style tests/busybox/date/date-timezone-colon-offset
run_old_style tests/busybox/date/date-u-works
run_old_style tests/busybox/date/date-works
run_old_style tests/busybox/date/date-works-1

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/dmesg/dmesg-invalid-option-fails
	run_old_style tests/busybox/dmesg/dmesg-n-requires-argument
	run_old_style tests/busybox/dmesg/dmesg-rejects-invalid-number
	run_old_style tests/busybox/dmesg/dmesg-reads-or-reports-permission-error
fi

run_old_style tests/busybox/dd/dd-copies-file
run_old_style tests/busybox/dd/dd-count-limits-copied-blocks
run_old_style tests/busybox/dd/dd-skip-and-seek
run_old_style tests/busybox/dd/dd-uses-stdin-and-stdout

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/df/df-kP-matches-host
	run_old_style tests/busybox/df/df-default-matches-host-kP
fi

run_old_style tests/busybox/du/du-k-matches-host
run_old_style tests/busybox/du/du-m-matches-host
run_old_style tests/busybox/du/du-h-matches-host
run_old_style tests/busybox/du/du-s-matches-host
run_old_style tests/busybox/du/du-l-matches-host

run_old_style tests/busybox/env/env-runs-command-with-assignment
run_old_style tests/busybox/env/env-unsets-variable

run_old_style tests/busybox/expr/expr-arithmetic
run_old_style tests/busybox/expr/expr-logical-ops
run_old_style tests/busybox/expr/expr-comparisons
run_old_style tests/busybox/expr/expr-big-integers

run_old_style tests/busybox/find/find-parenthesized-or
run_old_style tests/busybox/find/find-negated-name
run_old_style tests/busybox/find/find-supports-mindepth
run_old_style tests/busybox/find/find-prune-skips-matched-directory
run_old_style tests/busybox/find/find-delete-removes-matching-files
run_old_style tests/busybox/find/find-delete-removes-empty-directories-postorder

run_old_style tests/busybox/grep/grep-empty-pattern-argument-matches-all-lines
run_old_style tests/busybox/grep/grep-blank-line-pattern-file-matches-all-lines
run_old_style tests/busybox/grep/grep-counts-matching-lines
run_old_style tests/busybox/grep/grep-prints-line-numbers
run_old_style tests/busybox/grep/grep-files-with-matches
run_old_style tests/busybox/grep/grep-with-filename-on-single-input
run_old_style tests/busybox/grep/grep-no-filename-on-multiple-inputs

run_old_style tests/busybox/hexdump/hexdump-C-reads-stdin
run_old_style tests/busybox/hexdump/hexdump-C-renders-file
run_old_style tests/busybox/hexdump/hexdump-squeezes-repeated-lines
run_old_style tests/busybox/hexdump/hexdump-v-disables-squeezing
if [ "$is_linux" -eq 1 ]; then
run_old_style tests/busybox/ifconfig/ifconfig-lists-state-backed-interface
run_old_style tests/busybox/ifconfig/ifconfig-updates-address-and-mtu
run_old_style tests/busybox/ifconfig/ifconfig-adds-and-removes-inet4-state
run_old_style tests/busybox/ifconfig/ifconfig-adds-and-removes-inet6-state
run_old_style tests/busybox/ifconfig/ifconfig-updates-flags-metric-and-txqueuelen
run_old_style tests/busybox/ifconfig/ifconfig-updates-hardware-map-state
run_old_style tests/busybox/ip/ip-addr-show-prints-state
run_old_style tests/busybox/ip/ip-addr-add-preserves-existing-ipv4-state
run_old_style tests/busybox/ip/ip-addr-del-removes-only-target-state
run_old_style tests/busybox/ip/ip-addr-flush-dev-and-to-state
run_old_style tests/busybox/ip/ip-addr-show-and-flush-filter-label-and-scope-state
run_old_style tests/busybox/ip/ip-6-addr-show-filters-family
run_old_style tests/busybox/ip/ip-link-set-updates-state
run_old_style tests/busybox/ip/ip-link-set-flags-and-qlen-state
run_old_style tests/busybox/ip/ip-route-add-updates-state
run_old_style tests/busybox/ip/ip-route-adds-attributes-state
run_old_style tests/busybox/ip/ip-f-inet6-route-show-state
run_old_style tests/busybox/ip/ip-route-replace-and-flush-state
run_old_style tests/busybox/ip/ip-neigh-show-filters-state
run_old_style tests/busybox/ip/ip-neigh-flush-filters-state
run_old_style tests/busybox/ip/ip-rule-adds-and-deletes-state
run_old_style tests/busybox/ip/ip-6-route-add-updates-state
run_old_style tests/busybox/netstat/netstat-lnt-shows-listening-socket
run_old_style tests/busybox/netstat/netstat-plnt-shows-process
run_old_style tests/busybox/netstat/netstat-rn-prints-state-backed-route
run_old_style tests/busybox/netstat/netstat-6-rn-prints-state-backed-route
	run_old_style tests/busybox/ping/ping-c-1-reaches-loopback
	run_old_style tests/busybox/ping/ping-I-lo-reaches-loopback
	run_old_style tests/busybox/ping6/ping6-c-1-reaches-loopback
	run_old_style tests/busybox/ping6/ping6-I-lo-reaches-loopback
fi

run_old_style tests/busybox/stat/stat-c-prints-size-mode-and-name
run_old_style tests/busybox/stat/stat-c-reports-hard-links
run_old_style tests/busybox/stat/stat-c-reports-symlink

run_old_style tests/busybox/test/test-no-args-false
run_old_style tests/busybox/test/test-string-and-file-predicates
run_old_style tests/busybox/test/test-bang-and-boolean-ops
run_old_style tests/busybox/test/test-bracket-forms

run_old_style tests/busybox/time/time-runs-command-and-prints-timings
run_old_style tests/busybox/time/time-propagates-exit-status

run_old_style tests/busybox/unzip/unzip-extracts-files
run_old_style tests/busybox/unzip/unzip-d-extracts-into-directory
run_old_style tests/busybox/unzip/unzip-l-lists-entries
run_old_style tests/busybox/unzip/unzip-p-prints-selected-file
run_old_style tests/busybox/unzip/unzip-rejects-path-traversal

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/uptime/uptime-prints-linux-style-output
	run_old_style tests/busybox/uptime/uptime-matches-host-uptime-and-loads
fi

run_old_style tests/busybox/xargs/xargs-n-batches-arguments
run_old_style tests/busybox/xargs/xargs-p2-runs-jobs-in-parallel

run_old_style tests/busybox/less/less-prints-file
run_old_style tests/busybox/less/less-reads-stdin
run_old_style tests/busybox/less/less-concatenates-multiple-inputs
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/lsof/lsof-lists-open-passwd
	run_old_style tests/busybox/lsof/lsof-ignores-extra-operands

	run_old_style tests/busybox/lsmod/lsmod-prints-proc-modules
	run_old_style tests/busybox/lsmod/lsmod-rejects-extra-operand
fi

run_old_style tests/busybox/ps/ps-prints-header-and-self
run_old_style tests/busybox/ps/ps-lists-child-process
run_old_style tests/busybox/ps/ps-rejects-extra-operand

run_old_style tests/busybox/pgrep/pgrep-finds-child-process
run_old_style tests/busybox/pgrep/pgrep-does-not-match-script-name-in-wrapper-argv
run_old_style tests/busybox/pgrep/pgrep-exits-one-when-no-match
run_old_style tests/busybox/pgrep/pgrep-rejects-missing-pattern
run_old_style tests/busybox/pidof/pidof-finds-sleep-process
run_old_style tests/busybox/pidof/pidof-s-prints-one-pid
run_old_style tests/busybox/pidof/pidof-o-omits-pid
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/pivot_root/pivot_root-rejects-extra-operand
	run_old_style tests/busybox/poweroff/poweroff-w-exits-zero
fi

run_old_style tests/busybox/pkill/pkill-terminates-matching-child
run_old_style tests/busybox/pkill/pkill-does-not-kill-wrapper-shell-by-script-argv
run_old_style tests/busybox/pkill/pkill-exits-one-when-no-match
run_old_style tests/busybox/pkill/pkill-rejects-missing-pattern
run_old_style tests/busybox/pstree/pstree-p-shows-child-processes
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/switch_root/switch_root-dry-run-validates-target
	run_old_style tests/busybox/switch_root/switch_root-requires-pid1
fi

run_old_style tests/busybox/fsck/fsck-N-A-prints-delegated-checks
run_old_style tests/busybox/fsck/fsck-P-runs-same-pass-checks-in-parallel
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/hwclock/hwclock-rejects-unsupported-param-access
fi
run_old_style tests/busybox/mkswap/mkswap-writes-swapspace2-magic
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/mount/mount-f-a-reads-fstab
	run_old_style tests/busybox/mount/mount-runs-filesystem-helper-when-available
	run_old_style tests/busybox/mount/mount-i-skips-filesystem-helper
fi

run_old_style tests/busybox/killall/killall-terminates-exact-name-match
run_old_style tests/busybox/killall/killall-does-not-kill-wrapper-shell-by-script-argv
run_old_style tests/busybox/killall/killall-exits-one-when-no-match
run_old_style tests/busybox/killall/killall-rejects-missing-name
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/killall5/killall5-lists-signals
	run_old_style tests/busybox/killall5/killall5-rejects-bad-signal
fi

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/mountpoint/mountpoint-d-matches-host
	run_old_style tests/busybox/mountpoint/mountpoint-proc-is-mountpoint
	run_old_style tests/busybox/mountpoint/mountpoint-q-suppresses-output
	run_old_style tests/busybox/mountpoint/mountpoint-src-is-not-mountpoint
	run_old_style tests/busybox/mountpoint/mountpoint-symlink-nofollow
fi
run_old_style tests/busybox/stty/stty-F-raw-disables-canonical-and-echo
run_old_style tests/busybox/stty/stty-g-round-trips-state
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/swapoff/swapoff-a-reads-proc-swaps
	run_old_style tests/busybox/swapon/swapon-a-e-skips-missing-devices
fi

run_old_style tests/busybox/flock/flock-runs-command-under-lock
run_old_style tests/busybox/flock/flock-nonblock-fails-when-locked
run_old_style tests/busybox/flock/flock-c-runs-command-string

run_old_style tests/busybox/nslookup/nslookup-resolves-localhost
run_old_style tests/busybox/nslookup/nslookup-fails-unknown-host
run_old_style tests/busybox/nslookup/nslookup-rejects-missing-host

run_old_style tests/busybox/hostname/hostname-s-prints-short-hostname
run_old_style tests/busybox/hostname/hostname-rejects-extra-operands

run_old_style tests/busybox/patch/patch-applies-stdin-unified-diff
run_old_style tests/busybox/patch/patch-uses-new-path-when-old-missing
run_old_style tests/busybox/patch/patch-R-reverses-hunk
run_old_style tests/busybox/patch/patch-N-ignores-already-applied-hunk
run_old_style tests/busybox/patch/patch-file-operand-overrides-header
run_old_style tests/busybox/patch/patch-creates-new-file
run_old_style tests/busybox/patch/patch-p1-strips-leading-component
run_old_style tests/busybox/patch/patch-fails-without-partial-write
run_old_style tests/busybox/patch/patch-refuses-symlink-target

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/sysctl/sysctl-prints-keyed-values
	run_old_style tests/busybox/sysctl/sysctl-n-suppresses-keys
	run_old_style tests/busybox/sysctl/sysctl-fails-on-unknown-name
fi

run_old_style tests/busybox/setsid/setsid-creates-new-session
run_old_style tests/busybox/setsid/setsid-propagates-exit-status
run_old_style tests/busybox/setsid/setsid-rejects-missing-command

run_old_style tests/busybox/getopt/getopt-parses-short-options
run_old_style tests/busybox/getopt/getopt-parses-attached-argument
run_old_style tests/busybox/getopt/getopt-fails-invalid-option
run_old_style tests/busybox/getopt/getopt-fails-missing-option-argument

if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/halt/halt-w-exits-zero
fi

run_old_style tests/busybox/free/free-prints-memory-table
run_old_style tests/busybox/free/free-k-values-match-proc-meminfo
run_old_style tests/busybox/free/free-rejects-invalid-option

run_old_style tests/busybox/man/man-displays-manpage
run_old_style tests/busybox/man/man-fails-missing-page
run_old_style tests/busybox/man/man-rejects-missing-operand

run_old_style tests/busybox/watch/watch-runs-command-repeatedly
run_old_style tests/busybox/watch/watch-runs-shell-command-string
run_old_style tests/busybox/watch/watch-rejects-invalid-interval

run_old_style tests/busybox/run-parts/run-parts-runs-executable-scripts-in-order
run_old_style tests/busybox/run-parts/run-parts-skips-non-executable-files
run_old_style tests/busybox/run-parts/run-parts-test-lists-scripts-without-running-them
run_old_style tests/busybox/run-parts/run-parts-stops-on-failing-script

run_old_style tests/busybox/tree/tree-renders-directory-structure
run_old_style tests/busybox/tree/tree-renders-file-target

run_old_style tests/busybox/cp/cp-RHL-does_not_preserve-links
run_old_style tests/busybox/cp/cp-a-files-to-dir
run_old_style tests/busybox/cp/cp-a-preserves-links
run_old_style tests/busybox/cp/cp-copies-empty-file
run_old_style tests/busybox/cp/cp-copies-large-file
run_old_style tests/busybox/cp/cp-copies-small-file
run_old_style tests/busybox/cp/cp-d-files-to-dir
run_old_style tests/busybox/cp/cp-dev-file
run_old_style tests/busybox/cp/cp-dir-create-dir
run_old_style tests/busybox/cp/cp-dir-existing-dir
run_old_style tests/busybox/cp/cp-does-not-copy-unreadable-file
run_old_style tests/busybox/cp/cp-files-to-dir
run_old_style tests/busybox/cp/cp-follows-links
run_old_style tests/busybox/cp/cp-parents
run_old_style tests/busybox/cp/cp-preserves-hard-links
run_old_style tests/busybox/cp/cp-preserves-links
run_old_style tests/busybox/cp/cp-preserves-source-file
run_old_style tests/busybox/cp/cp-refuses-same-file
run_old_style tests/busybox/cp/cp-T-copies-into-target-directory-itself
run_old_style tests/busybox/find/find-supports-minus-xdev

run_old_style tests/busybox/gzip/gzip-accepts-single-minus
run_old_style tests/busybox/gzip/gzip-accepts-multiple-files
run_old_style tests/busybox/gzip/gzip-compression-levels
run_old_style tests/busybox/gzip/gzip-removes-original-file

run_old_style tests/busybox/install/install-copies-file
run_old_style tests/busybox/install/install-D-creates-parent-directories
run_old_style tests/busybox/install/install-d-creates-directories
run_old_style tests/busybox/install/install-m-sets-mode
run_old_style tests/busybox/install/install-refuses-symlink-target
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/insmod/insmod-rejects-missing-module
	run_old_style tests/busybox/insmod/insmod-reports-missing-file
	run_old_style tests/busybox/insmod/insmod-rejects-empty-file
fi

run_old_style tests/busybox/ln/ln-creates-hard-link
run_old_style tests/busybox/ln/ln-creates-link-in-directory
run_old_style tests/busybox/ln/ln-symbolic-link
run_old_style tests/busybox/ln/ln-force-replaces-existing-target
run_old_style tests/busybox/ln/ln-T-does-not-create-link-inside-directory

run_old_style tests/busybox/mkfifo/mkfifo-creates-fifo
run_old_style tests/busybox/mkfifo/mkfifo-creates-multiple-fifos
run_old_style tests/busybox/mkfifo/mkfifo-m-sets-mode

run_old_style tests/busybox/split/split-by-lines
run_old_style tests/busybox/split/split-by-bytes

run_old_style tests/busybox/head/head-prints-first-lines
run_old_style tests/busybox/head/head-prints-first-bytes
run_old_style tests/busybox/head/head-omits-last-line-with-negative-count

run_old_style tests/busybox/tar/tar-archives-multiple-files
run_old_style tests/busybox/tar/tar--overwrite-preserves-hardlinks
run_old_style tests/busybox/tar/tar-complains-about-missing-file
run_old_style tests/busybox/tar/tar-demands-at-least-one-ctx
run_old_style tests/busybox/tar/tar-demands-at-most-one-ctx
run_old_style tests/busybox/tar/tar-does-not-extract-into-symlinks
run_old_style tests/busybox/tar/tar-does-not-create-through-symlinked-parent
run_old_style tests/busybox/tar/tar-extract-tgz
run_old_style tests/busybox/tar/tar-extract-txz
run_old_style tests/busybox/tar/tar-extracts-all-subdirs
run_old_style tests/busybox/tar/tar-extracts-file
run_old_style tests/busybox/tar/tar-extracts-from-standard-input
run_old_style tests/busybox/tar/tar-extracts-multiple-files
run_old_style tests/busybox/tar/tar-extracts-to-standard-output
run_old_style tests/busybox/tar/tar-handles-cz-options
run_old_style tests/busybox/tar/tar-handles-empty-include-and-non-empty-exclude-list
run_old_style tests/busybox/tar/tar-handles-exclude-and-extract-lists
run_old_style tests/busybox/tar/tar-handles-multiple-X-options
run_old_style tests/busybox/tar/tar-handles-nested-exclude
run_old_style tests/busybox/tar/tar-hardlinks-and-repeated-files
run_old_style tests/busybox/tar/tar-hardlinks-mode
run_old_style tests/busybox/tar/tar-k-does-not-extract-into-symlinks
run_old_style tests/busybox/tar/tar-lists-pax-long-paths
run_old_style tests/busybox/tar/tar-overwrite-does-not-write-through-symlinked-parent
run_old_style tests/busybox/tar/tar-pax-utf8-names-and-symlinks
run_old_style tests/busybox/tar/tar-supports-strip-components
run_old_style tests/busybox/tar/tar-symlink-attack-is-contained
run_old_style tests/busybox/tar/tar-symlinks-mode
run_old_style tests/busybox/tar/tar-symlinks-and-hardlinks-coexist
run_old_style tests/busybox/tar/tar-strips-dotdot-components
run_old_style tests/busybox/tar/tar-writing-into-read-only-dir
run_old_style tests/busybox/tar/tar_with_link_with_size
run_old_style tests/busybox/tar/tar_with_prefix_fields

run_old_style_ls tests/busybox/ls/ls-1-works
run_old_style_ls tests/busybox/ls/ls-double-dash-works
run_old_style_ls tests/busybox/ls/ls-h-works
run_old_style_ls tests/busybox/ls/ls-l-works
run_old_style_ls tests/busybox/ls/ls-l-symlink-to-dir-works
run_old_style_ls tests/busybox/ls/ls-multiple-targets-works
run_old_style_ls tests/busybox/ls/ls-s-works
run_old_style_ls tests/busybox/ls/ls-symlink-to-dir-works

run_old_style tests/busybox/seq/seq-counts-up
run_old_style tests/busybox/seq/seq-supports-equal-width
run_old_style tests/busybox/seq/seq-supports-custom-separator

run_old_style tests/busybox/readlink/readlink-prints-link-target
run_old_style tests/busybox/readlink/readlink-f-canonicalizes
run_old_style tests/busybox/readlink/readlink-f-normalizes-dotdot-after-missing-component
run_old_style tests/busybox/realpath/realpath-canonicalizes-existing-path
run_old_style tests/busybox/realpath/realpath-fails-for-missing-path
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/rfkill/rfkill-list-reports-missing-device-or-lists
	run_old_style tests/busybox/rfkill/rfkill-list-rejects-invalid-target
	run_old_style tests/busybox/rfkill/rfkill-block-requires-target
	run_old_style tests/busybox/reboot/reboot-w-exits-zero
	run_old_style tests/busybox/reboot/reboot-d-requires-argument
fi

run_old_style tests/busybox/tail/tail-prints-last-lines
run_old_style tests/busybox/tail/tail-prints-from-line
run_old_style tests/busybox/tail/tail-prints-last-bytes

run_old_style tests/busybox/touch/touch-creates-file
run_old_style tests/busybox/touch/touch-c-no-create
run_old_style tests/busybox/touch/touch-r-copies-reference-time
run_old_style tests/busybox/timeout/timeout-allows-short-command
run_old_style tests/busybox/timeout/timeout-kills-long-command
run_old_style tests/busybox/timeout/timeout-kills-term-ignoring-process-group
run_old_style tests/busybox/tr/tr-translates-range
run_old_style tests/busybox/tr/tr-deletes-set
run_old_style tests/busybox/tr/tr-squeezes-set

run_old_style tests/busybox/uniq/uniq-collapses-adjacent-lines
run_old_style tests/busybox/uniq/uniq-counts-groups
run_old_style tests/busybox/uniq/uniq-ignores-case

run_old_style tests/busybox/mkdir/mkdir-makes-a-directory
run_old_style tests/busybox/mkdir/mkdir-makes-parent-directories

run_old_style tests/busybox/paste/paste-parallel-files
run_old_style tests/busybox/paste/paste-serial-mode
run_old_style tests/busybox/paste/paste-custom-delimiter

run_old_style tests/busybox/mv/mv-files-to-dir
run_old_style tests/busybox/mv/mv-files-to-dir-2
run_old_style tests/busybox/mv/mv-follows-links
run_old_style tests/busybox/mv/mv-moves-empty-file
run_old_style tests/busybox/mv/mv-moves-file
run_old_style tests/busybox/mv/mv-moves-hardlinks
run_old_style tests/busybox/mv/mv-moves-large-file
run_old_style tests/busybox/mv/mv-moves-small-file
run_old_style tests/busybox/mv/mv-moves-symlinks
run_old_style tests/busybox/mv/mv-moves-unreadable-files
run_old_style tests/busybox/mv/mv-preserves-hard-links
run_old_style tests/busybox/mv/mv-preserves-links
run_old_style tests/busybox/mv/mv-refuses-mv-dir-to-subdir
run_old_style tests/busybox/mv/mv-removes-source-file
run_old_style tests/busybox/mv/mv-T-does-not-move-into-directory

run_old_style tests/busybox/nologin/nologin-exits-nonzero-and-prints-default-message
run_old_style tests/busybox/nologin/nologin-rejects-extra-operand

run_old_style tests/busybox/rm/rm-removes-file
run_old_style tests/busybox/rmdir/rmdir-removes-parent-directories
if [ "$is_linux" -eq 1 ]; then
	run_old_style tests/busybox/rmmod/rmmod-rejects-missing-module
	run_old_style tests/busybox/rmmod/rmmod-rejects-invalid-option
	run_old_style tests/busybox/rmmod/rmmod-a-fails-cleanly-without-privilege
fi
run_old_style tests/busybox/sleep/sleep-rejects-invalid-interval
run_old_style tests/busybox/sleep/sleep-sums-arguments
run_old_style tests/busybox/tee/tee-appends-input
run_old_style tests/busybox/tee/tee-tees-input
run_old_style tests/busybox/uname/uname-machine-matches-host
run_old_style tests/busybox/uname/uname-sysname-matches-host
run_old_style tests/busybox/wc/wc-counts-all
run_old_style tests/busybox/wc/wc-counts-characters
run_old_style tests/busybox/wc/wc-counts-lines
run_old_style tests/busybox/wc/wc-counts-words
run_old_style tests/busybox/wc/wc-prints-longest-line-length
run_old_style_internet tests/busybox/wget/wget-retrieves-google-index
run_old_style_internet tests/busybox/wget/wget--O-overrides--P
run_old_style_internet tests/busybox/wget/wget-supports--P
run_old_style_internet tests/busybox/wget/wget-handles-empty-path
