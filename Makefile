XDG_CACHE_HOME := $(CURDIR)/.cache

build:
	cargo build

setup:
	XDG_CACHE_HOME=$(XDG_CACHE_HOME) prek install --install-hooks

pc:
	XDG_CACHE_HOME=$(XDG_CACHE_HOME) prek --quiet run --all-files
