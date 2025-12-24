#!/bin/bash
set -e

# Source cargo environment if it exists
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

# Change to the repository root where Cargo.toml is located
cd "$(git rev-parse --show-toplevel)"

# Run clippy with strict options
cargo clippy --all-targets --all-features -- \
    -D warnings \
    -W clippy::pedantic \
    -W clippy::nursery \
    -W clippy::cargo \
    -A clippy::must_use_candidate \
    -A clippy::missing_errors_doc \
    -A clippy::missing_panics_doc

