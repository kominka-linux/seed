#!/bin/sh

set -eu

repo_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
binary="$repo_dir/target/debug/seed"
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

for applet in busybox bunzip2 bzip2 bzcat cat chmod chgrp chown chroot cmp cp date dd df diff du egrep env expr find flock free getopt grep gunzip gzip hexdump install killall less ln ls lzcat lzma man mkdir mkfifo mountpoint mv nslookup od paste patch pgrep pkill printf ps readlink realpath rm rmdir run-parts setsid sleep sort split stat sysctl tar tee test time touch timeout tr tree uname unlzma unxz unzip uptime watch wc wget xargs xz xzcat zcat '[' '[['; do
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

run_old_style tests/busybox/basename/basename-strips-suffix
run_old_style tests/busybox/basename/basename-multiple-names
run_old_style tests/busybox/basename/dirname-basic

run_old_style tests/busybox/chmod/chmod-R-descends-before-changing-directory
run_old_style tests/busybox/chgrp/chgrp-sets-group
run_old_style tests/busybox/chown/chown-sets-owner
run_old_style tests/busybox/chown/chown-sets-owner-and-group
run_old_style tests/busybox/chroot/chroot-propagates-command-exit-status
run_old_style tests/busybox/chroot/chroot-rejects-missing-root
run_old_style tests/busybox/chroot/chroot-runs-command-in-new-root
run_old_style tests/busybox/chroot/chroot-fails-missing-root-path

run_old_style tests/busybox/cmp/cmp-identical-files
run_old_style tests/busybox/cmp/cmp-different-files-exit-one
run_old_style tests/busybox/cmp/cmp-reads-stdin
run_old_style tests/busybox/cmp/cmp-s-detects-difference

run_old_style tests/busybox/cut/cut-selects-bytes
run_old_style tests/busybox/cut/cut-selects-fields
run_old_style tests/busybox/cut/cut-suppresses-undelimited-lines

run_old_style tests/busybox/date/date-@-works
run_old_style tests/busybox/date/date-D-works
run_old_style tests/busybox/date/date-I-works
run_old_style tests/busybox/date/date-R-works
run_old_style tests/busybox/date/date-format-works
run_old_style tests/busybox/date/date-timezone
run_old_style tests/busybox/date/date-u-works
run_old_style tests/busybox/date/date-works
run_old_style tests/busybox/date/date-works-1

run_old_style tests/busybox/dd/dd-copies-file
run_old_style tests/busybox/dd/dd-count-limits-copied-blocks
run_old_style tests/busybox/dd/dd-skip-and-seek
run_old_style tests/busybox/dd/dd-uses-stdin-and-stdout

run_old_style tests/busybox/df/df-kP-matches-host
run_old_style tests/busybox/df/df-default-matches-host-kP

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

run_old_style tests/busybox/hexdump/hexdump-C-reads-stdin
run_old_style tests/busybox/hexdump/hexdump-C-renders-file
run_old_style tests/busybox/hexdump/hexdump-squeezes-repeated-lines
run_old_style tests/busybox/hexdump/hexdump-v-disables-squeezing

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

run_old_style tests/busybox/uptime/uptime-prints-linux-style-output
run_old_style tests/busybox/uptime/uptime-matches-host-uptime-and-loads

run_old_style tests/busybox/xargs/xargs-n-batches-arguments
run_old_style tests/busybox/xargs/xargs-p2-runs-jobs-in-parallel

run_old_style tests/busybox/less/less-prints-file
run_old_style tests/busybox/less/less-reads-stdin
run_old_style tests/busybox/less/less-concatenates-multiple-inputs

run_old_style tests/busybox/ps/ps-prints-header-and-self
run_old_style tests/busybox/ps/ps-lists-child-process
run_old_style tests/busybox/ps/ps-rejects-extra-operand

run_old_style tests/busybox/pgrep/pgrep-finds-child-process
run_old_style tests/busybox/pgrep/pgrep-exits-one-when-no-match
run_old_style tests/busybox/pgrep/pgrep-rejects-missing-pattern

run_old_style tests/busybox/pkill/pkill-terminates-matching-child
run_old_style tests/busybox/pkill/pkill-exits-one-when-no-match
run_old_style tests/busybox/pkill/pkill-rejects-missing-pattern

run_old_style tests/busybox/killall/killall-terminates-exact-name-match
run_old_style tests/busybox/killall/killall-exits-one-when-no-match
run_old_style tests/busybox/killall/killall-rejects-missing-name

run_old_style tests/busybox/mountpoint/mountpoint-d-matches-host
run_old_style tests/busybox/mountpoint/mountpoint-proc-is-mountpoint
run_old_style tests/busybox/mountpoint/mountpoint-q-suppresses-output
run_old_style tests/busybox/mountpoint/mountpoint-src-is-not-mountpoint
run_old_style tests/busybox/mountpoint/mountpoint-symlink-nofollow

run_old_style tests/busybox/flock/flock-runs-command-under-lock
run_old_style tests/busybox/flock/flock-nonblock-fails-when-locked
run_old_style tests/busybox/flock/flock-c-runs-command-string

run_old_style tests/busybox/nslookup/nslookup-resolves-localhost
run_old_style tests/busybox/nslookup/nslookup-fails-unknown-host
run_old_style tests/busybox/nslookup/nslookup-rejects-missing-host

run_old_style tests/busybox/patch/patch-applies-stdin-unified-diff
run_old_style tests/busybox/patch/patch-uses-new-path-when-old-missing
run_old_style tests/busybox/patch/patch-R-reverses-hunk
run_old_style tests/busybox/patch/patch-N-ignores-already-applied-hunk
run_old_style tests/busybox/patch/patch-file-operand-overrides-header
run_old_style tests/busybox/patch/patch-creates-new-file
run_old_style tests/busybox/patch/patch-p1-strips-leading-component
run_old_style tests/busybox/patch/patch-fails-without-partial-write
run_old_style tests/busybox/patch/patch-refuses-symlink-target

run_old_style tests/busybox/sysctl/sysctl-prints-keyed-values
run_old_style tests/busybox/sysctl/sysctl-n-suppresses-keys
run_old_style tests/busybox/sysctl/sysctl-fails-on-unknown-name

run_old_style tests/busybox/setsid/setsid-creates-new-session
run_old_style tests/busybox/setsid/setsid-propagates-exit-status
run_old_style tests/busybox/setsid/setsid-rejects-missing-command

run_old_style tests/busybox/getopt/getopt-parses-short-options
run_old_style tests/busybox/getopt/getopt-parses-attached-argument
run_old_style tests/busybox/getopt/getopt-fails-invalid-option
run_old_style tests/busybox/getopt/getopt-fails-missing-option-argument

run_old_style tests/busybox/free/free-prints-memory-table
run_old_style tests/busybox/free/free-k-values-match-proc-meminfo
run_old_style tests/busybox/free/free-rejects-invalid-option

run_old_style tests/busybox/man/man-displays-manpage
run_old_style tests/busybox/man/man-fails-missing-page
run_old_style tests/busybox/man/man-rejects-missing-operand

run_old_style tests/busybox/watch/watch-runs-command-repeatedly
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

run_old_style tests/busybox/ln/ln-creates-hard-link
run_old_style tests/busybox/ln/ln-creates-link-in-directory
run_old_style tests/busybox/ln/ln-symbolic-link
run_old_style tests/busybox/ln/ln-force-replaces-existing-target

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
run_old_style tests/busybox/realpath/realpath-canonicalizes-existing-path
run_old_style tests/busybox/realpath/realpath-fails-for-missing-path

run_old_style tests/busybox/tail/tail-prints-last-lines
run_old_style tests/busybox/tail/tail-prints-from-line
run_old_style tests/busybox/tail/tail-prints-last-bytes

run_old_style tests/busybox/touch/touch-creates-file
run_old_style tests/busybox/touch/touch-c-no-create
run_old_style tests/busybox/touch/touch-r-copies-reference-time
run_old_style tests/busybox/timeout/timeout-allows-short-command
run_old_style tests/busybox/timeout/timeout-kills-long-command
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

run_old_style tests/busybox/rm/rm-removes-file
run_old_style tests/busybox/rmdir/rmdir-removes-parent-directories
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
