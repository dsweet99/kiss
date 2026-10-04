.DEFAULT_GOAL := all

.PHONY: all install test lint clean

# Job count and memory limits come from .cargo/config.toml: admin/rustc_memlock.sh
# shared-locks every rustc up to build.jobs.
# release-local is the release profile plus incremental compilation (Cargo.toml).
all:
	cargo build --profile release-local

install:
	cargo install --path . --force --locked --config 'build.rustflags=[]'

test:
	pytest tests && cargo nextest run

lint:
	$(HOME)/kiss-tmp check
	ruff check .
	cargo clippy --all-targets --all-features -- -D warnings -W clippy::cargo

clean:
	cargo clean
