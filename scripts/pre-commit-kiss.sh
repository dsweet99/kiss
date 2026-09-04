#!/bin/bash
set -e

# Use the repo-local `kiss` binary (release build; not a globally installed one).
# cargo run --quiet --release -- check --ignore fake_
kiss check
