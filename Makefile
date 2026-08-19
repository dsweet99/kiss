.PHONY: all clean test lint install

all:
	cargo build --release

test:
	pytest tests && cargo nextest run

lint:
	$(HOME)/kiss-tmp check
	ruff check .
	cargo clippy --all-targets --all-features -- -D warnings -W clippy::cargo

install:
	cargo install --path . --force

clean:
	cargo clean

