#!/bin/sh

set -eu

repo_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
image_tag=seed-phase7b:alpine
privileged=0

while [ $# -gt 0 ]; do
	case "$1" in
		--privileged)
			privileged=1
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

if [ $# -eq 0 ]; then
	set -- /src/tests/linux/phase7b-applets.sh
fi

docker build -q -f "$repo_dir/tests/linux/Dockerfile.phase7b" -t "$image_tag" "$repo_dir" >/dev/null

if [ "$privileged" -eq 1 ]; then
	docker run --rm -t --privileged \
		-v "$repo_dir:/src:ro" \
		-v seed-phase7b-cargo-registry:/cargo/registry \
		-v seed-phase7b-cargo-git:/cargo/git \
		-v seed-phase7b-target:/target \
		-w /src \
		-e CARGO_HOME=/cargo \
		-e CARGO_TARGET_DIR=/target \
		-e SEED_REQUIRE_LOOP=1 \
		"$image_tag" \
		"$@"
else
	docker run --rm -t \
		-v "$repo_dir:/src:ro" \
		-v seed-phase7b-cargo-registry:/cargo/registry \
		-v seed-phase7b-cargo-git:/cargo/git \
		-v seed-phase7b-target:/target \
		-w /src \
		-e CARGO_HOME=/cargo \
		-e CARGO_TARGET_DIR=/target \
		"$image_tag" \
		"$@"
fi
