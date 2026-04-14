#!/bin/sh

set -eu

cd /work

cargo build --quiet

binary="${CARGO_TARGET_DIR:-target}/debug/seed"
tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/seed-init-linux.XXXXXX")
trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

wait_for_file() {
	path=$1
	attempts=${2:-100}
	while [ "$attempts" -gt 0 ]; do
		if [ -e "$path" ]; then
			return 0
		fi
		attempts=$((attempts - 1))
		sleep 0.05
	done
	return 1
}

wait_for_child_pid() {
	parent_pid=$1
	attempts=${2:-100}
	while [ "$attempts" -gt 0 ]; do
		children_file="/proc/$parent_pid/task/$parent_pid/children"
		child_pid=
		if [ -r "$children_file" ]; then
			child_pid=$(tr ' ' '\n' <"$children_file" | sed -n '1p')
		fi
		if [ -n "${child_pid:-}" ]; then
			printf '%s\n' "$child_pid"
			return 0
		fi
		attempts=$((attempts - 1))
		sleep 0.05
	done
	return 1
}

run_pid1() {
	inittab=$1
	shift
	env SEED_INIT_INITTAB="$inittab" "$@" \
		unshare -pf --mount-proc "$binary" init
}

test_pid1_runs_sysinit_and_respawn() {
	case_dir="$tmpdir/pid1-smoke"
	mkdir -p "$case_dir"

	cat >"$case_dir/sysinit.sh" <<'EOF'
#!/bin/sh
	printf 'sysinit\n' >>"$1"
EOF
	chmod +x "$case_dir/sysinit.sh"

	cat >"$case_dir/respawn.sh" <<'EOF'
#!/bin/sh
	printf 'respawn\n' >>"$1"
EOF
	chmod +x "$case_dir/respawn.sh"

	cat >"$case_dir/inittab" <<EOF
::sysinit:$case_dir/sysinit.sh $case_dir/sysinit.log
::respawn:$case_dir/respawn.sh $case_dir/respawn.log
EOF

	run_pid1 "$case_dir/inittab" \
		SEED_INIT_TEST_EXIT_WHEN_IDLE=1 \
		SEED_INIT_TEST_MAX_RESPAWNS=2

	test "$(wc -l <"$case_dir/sysinit.log")" -eq 1
	test "$(wc -l <"$case_dir/respawn.log")" -eq 2
}

test_respawn_is_rate_limited() {
	case_dir="$tmpdir/rate-limit"
	mkdir -p "$case_dir"

	cat >"$case_dir/respawn.sh" <<'EOF'
#!/bin/sh
python3 - "$1" <<'PY'
import pathlib
import sys
import time

path = pathlib.Path(sys.argv[1])
with path.open("a", encoding="utf-8") as handle:
    handle.write(f"{time.time_ns() // 1_000_000}\n")
PY
EOF
	chmod +x "$case_dir/respawn.sh"

	cat >"$case_dir/inittab" <<EOF
::respawn:$case_dir/respawn.sh $case_dir/timestamps.log
EOF

	run_pid1 "$case_dir/inittab" \
		SEED_INIT_TEST_EXIT_WHEN_IDLE=1 \
		SEED_INIT_TEST_MAX_RESPAWNS=3

	python3 - "$case_dir/timestamps.log" <<'PY'
import pathlib
import sys

values = [int(line.strip()) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]
assert len(values) == 3, values
gaps = [later - earlier for earlier, later in zip(values, values[1:])]
assert min(gaps) >= 900, gaps
PY
}

test_shutdown_kills_stubborn_process_group() {
	case_dir="$tmpdir/shutdown"
	mkdir -p "$case_dir"

	cat >"$case_dir/respawn.sh" <<'EOF'
#!/bin/sh
pid_file=$1
child_file=$2
(trap '' TERM INT; while :; do sleep 1; done) &
child=$!
printf '%s\n' "$$" >"$pid_file"
printf '%s\n' "$child" >"$child_file"
trap '' TERM INT
while :; do sleep 1; done
EOF
	chmod +x "$case_dir/respawn.sh"

	cat >"$case_dir/shutdown.sh" <<'EOF'
#!/bin/sh
pid=$(cat "$1")
child=$(cat "$2")
python3 - "$pid" "$child" <<'PY'
import os
import signal
import sys
import time

pids = [int(value) for value in sys.argv[1:]]
deadline = time.time() + 5
while time.time() < deadline:
    alive = []
    for pid in pids:
        try:
            os.kill(pid, 0)
        except OSError:
            continue
        alive.append(pid)
    if not alive:
        sys.exit(0)
    time.sleep(0.1)
sys.exit(1)
PY
	printf 'shutdown\n' >"$3"
EOF
	chmod +x "$case_dir/shutdown.sh"

	cat >"$case_dir/inittab" <<EOF
::shutdown:$case_dir/shutdown.sh $case_dir/respawn.pid $case_dir/grandchild.pid $case_dir/shutdown.log
::respawn:$case_dir/respawn.sh $case_dir/respawn.pid $case_dir/grandchild.pid
EOF

	env SEED_INIT_INITTAB="$case_dir/inittab" \
		SEED_INIT_TEST_ACTION_LOG="$case_dir/action.log" \
		unshare -pf --mount-proc "$binary" init &
	unshare_pid=$!

	wait_for_file "$case_dir/grandchild.pid"
	init_pid=$(wait_for_child_pid "$unshare_pid")
	kill -USR1 "$init_pid"
	wait "$unshare_pid"

	grep '^shutdown$' "$case_dir/shutdown.log" >/dev/null
	grep '^halt$' "$case_dir/action.log" >/dev/null
}

test_restart_kills_stubborn_process_group() {
	case_dir="$tmpdir/restart"
	mkdir -p "$case_dir"

	cat >"$case_dir/respawn.sh" <<'EOF'
#!/bin/sh
pid_file=$1
child_file=$2
(trap '' TERM INT; while :; do sleep 1; done) &
child=$!
printf '%s\n' "$$" >"$pid_file"
printf '%s\n' "$child" >"$child_file"
trap '' TERM INT
while :; do sleep 1; done
EOF
	chmod +x "$case_dir/respawn.sh"

	cat >"$case_dir/restart.sh" <<'EOF'
#!/bin/sh
pid=$(cat "$1")
child=$(cat "$2")
python3 - "$pid" "$child" <<'PY'
import os
import sys
import time

pids = [int(value) for value in sys.argv[1:]]
deadline = time.time() + 5
while time.time() < deadline:
    alive = []
    for pid in pids:
        try:
            os.kill(pid, 0)
        except OSError:
            continue
        alive.append(pid)
    if not alive:
        sys.exit(0)
    time.sleep(0.1)
sys.exit(1)
PY
	printf 'restart\n' >"$3"
EOF
	chmod +x "$case_dir/restart.sh"

	cat >"$case_dir/inittab" <<EOF
::restart:$case_dir/restart.sh $case_dir/respawn.pid $case_dir/grandchild.pid $case_dir/restart.log
::respawn:$case_dir/respawn.sh $case_dir/respawn.pid $case_dir/grandchild.pid
EOF

	env SEED_INIT_INITTAB="$case_dir/inittab" \
		unshare -pf --mount-proc "$binary" init &
	unshare_pid=$!

	wait_for_file "$case_dir/grandchild.pid"
	init_pid=$(wait_for_child_pid "$unshare_pid")
	kill -HUP "$init_pid"
	wait "$unshare_pid"

	grep '^restart$' "$case_dir/restart.log" >/dev/null
}

test_respawn_binds_real_pty() {
	case_dir="$tmpdir/pty"
	mkdir -p "$case_dir/dev"

	python3 - "$case_dir" <<'PY' &
import os
import pty
import select
import sys

case_dir = sys.argv[1]
master, slave = pty.openpty()
with open(f"{case_dir}/slave.path", "w", encoding="utf-8") as handle:
    handle.write(os.ttyname(slave) + "\n")
with open(f"{case_dir}/tty.log", "wb", buffering=0) as log:
    while True:
        if os.path.exists(f"{case_dir}/stop"):
            break
        ready, _, _ = select.select([master], [], [], 0.1)
        if master in ready:
            try:
                data = os.read(master, 4096)
            except OSError:
                break
            if not data:
                break
            log.write(data)
os.close(slave)
os.close(master)
PY
	logger_pid=$!

	wait_for_file "$case_dir/slave.path"
	slave_path=$(cat "$case_dir/slave.path")
	ln -sf "$slave_path" "$case_dir/dev/tty1"

	cat >"$case_dir/inittab" <<EOF
tty1::respawn:sh -c 'test -t 0 && tty && printf hello-pty\\n'
EOF

	run_pid1 "$case_dir/inittab" \
		SEED_INIT_TEST_EXIT_WHEN_IDLE=1 \
		SEED_INIT_TEST_MAX_RESPAWNS=1 \
		SEED_INIT_TTY_DIR="$case_dir/dev"

	: >"$case_dir/stop"
	wait "$logger_pid"

	grep '/dev/pts/' "$case_dir/tty.log" >/dev/null
	grep 'hello-pty' "$case_dir/tty.log" >/dev/null
}

test_pid1_runs_sysinit_and_respawn
test_respawn_is_rate_limited
test_shutdown_kills_stubborn_process_group
test_restart_kills_stubborn_process_group
test_respawn_binds_real_pty

printf 'linux-init: ok\n'
