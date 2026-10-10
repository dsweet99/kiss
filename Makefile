.DEFAULT_GOAL := all

.PHONY: all install test lint clean

# Job count and the rustc wrapper (admin/rustc_sccache.sh) come from .cargo/config.toml.
# release-local is the release profile plus incremental compilation (Cargo.toml).
all:
	cargo build --profile release-local

install:
	cargo install --path . --force --locked --config 'build.rustflags=[]'

test:
	pytest tests && cargo nextest run

lint:
	kiss check
	ruff check .
	cargo clippy --all-targets --all-features -- -D warnings -W clippy::cargo

clean:
	cargo clean
