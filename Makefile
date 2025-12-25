.PHONY: all clean test install

all:
	cargo build --release

test:
	cargo test

install:
	cargo install --path . --force

clean:
	cargo clean

