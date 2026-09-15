.PHONY: all clean test lint install

all:
	cargo build --release

# Prefer tmpfs for TempDir publish fsync (ext4 /tmp fsync dominates per-test SLA).
test:
	pytest tests && TMPDIR=/dev/shm cargo nextest run

lint:
	$(HOME)/kiss-tmp check
	ruff check .
	cargo clippy --all-targets --all-features -- -D warnings -W clippy::cargo

install:
	cargo install --path . --force

clean:
	cargo clean

