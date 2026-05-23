.PHONY: all clean test install

all:
	cargo build --release

test:
	pytest tests && cargo nextest run

install:
	cargo install --path . --force

clean:
	cargo clean

