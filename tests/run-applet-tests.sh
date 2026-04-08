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

run_old_style() {
	test_script=$1
	tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-test.XXXXXX")
	printf 'old-style: %s\n' "$test_script"
	(
		cd "$tmpdir"
		PATH="$links_dir:$PATH" sh "$repo_dir/$test_script"
	)
	rm -rf "$tmpdir"
}

mkdir -p "$links_dir"
cargo build --quiet --manifest-path "$repo_dir/Cargo.toml"

for applet in busybox cat chmod cp diff egrep grep mkdir mv od printf rm rmdir sort tee wc; do
	ln -sf "$binary" "$links_dir/$applet"
done

echo_ne=$(mktemp "${TMPDIR:-/tmp}/seed-echo-ne.XXXXXX")
trap 'rm -f "$echo_ne"' EXIT HUP INT TERM
make_echo_ne "$echo_ne"

run_new_style cat.tests ':FEATURE_CATV:FEATURE_CATN:'
run_new_style cp.tests
run_new_style diff.tests ':FEATURE_DIFF_DIR:'
run_new_style grep.tests ':EXTRA_COMPAT:EGREP:'
run_new_style od.tests ':DESKTOP:LONG_OPTS:'
run_new_style printf.tests
run_new_style sort.tests ':FEATURE_SORT_BIG:'

run_old_style tests/busybox/cat/cat-prints-a-file
run_old_style tests/busybox/cat/cat-prints-a-file-and-standard-input

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

run_old_style tests/busybox/mkdir/mkdir-makes-a-directory
run_old_style tests/busybox/mkdir/mkdir-makes-parent-directories

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
run_old_style tests/busybox/tee/tee-appends-input
run_old_style tests/busybox/tee/tee-tees-input
run_old_style tests/busybox/wc/wc-counts-all
run_old_style tests/busybox/wc/wc-counts-characters
run_old_style tests/busybox/wc/wc-counts-lines
run_old_style tests/busybox/wc/wc-counts-words
run_old_style tests/busybox/wc/wc-prints-longest-line-length
