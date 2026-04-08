XDG_CACHE_HOME := $(CURDIR)/.cache

build:
	cargo build

test:
	cargo test --quiet
	./tests/run-applet-tests.sh

test-applets:
	./tests/run-applet-tests.sh

test-phase7b-linux:
	./tests/run-linux-phase7b.sh

test-phase7b-linux-privileged:
	./tests/run-linux-phase7b.sh --privileged

setup:
	XDG_CACHE_HOME=$(XDG_CACHE_HOME) prek install --install-hooks

pc:
	XDG_CACHE_HOME=$(XDG_CACHE_HOME) prek --quiet run --all-files
