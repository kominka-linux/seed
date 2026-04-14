#!/bin/sh

set -eu

repo_dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

docker compose run --rm dev-privileged sh /work/tests/linux/init-hardening.sh
