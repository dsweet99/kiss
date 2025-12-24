.PHONY: all clean test

all:
	cargo build --release

test:
	cargo test

clean:
	cargo clean

