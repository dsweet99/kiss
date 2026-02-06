#!/bin/bash
set -e

# Use the repo-local `kiss` binary (not a globally installed one).
cargo run --quiet -- check --ignore fake_

