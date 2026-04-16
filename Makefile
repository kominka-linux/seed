XDG_CACHE_HOME := $(CURDIR)/.cache

build:
	cargo build

build-linux-arm64:
	docker compose run --rm dev sh -lc ' \
		set -eu; \
		target=$$(rustc -vV | sed -n "s/^host: //p"); \
		cargo build --quiet --target "$$target" --bin seed; \
		mkdir -p "/work/target/$$target/debug"; \
		install -m 755 "/cargo-target/$$target/debug/seed" "/work/target/$$target/debug/seed"; \
	'

manpages:
	mkdir -p docs/man
	rm -f docs/man/*.1
	for applet in $$(cargo run --quiet --features docgen --bin seed-docgen -- list); do \
		cargo run --quiet --features docgen --bin seed-docgen -- man "$$applet" > "docs/man/$$applet.1"; \
	done

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
