# Changelog

## 0.4.11 — 2026-09-17

Release of the `dsweet/iml_2` line (version bump since 0.4.10 on crates.io).

### Highlights

- Faster, size-balanced multi-shard `kiss check` (more shards, lightest-bin packing by file size).
- `kiss test --watch` and `kiss test --retry-bad`, plus broader coverage/cache and tool-identity work for colder/warmer runs.
- Orphan detection moved onto the `kiss test` path (`[test] orphan_detection`); `kiss check` no longer reports orphans.
- Eval harness under `evals/` and `ops/evaluate.py` for repeatable timing and behavior metrics.
- README documents `kiss test` external toolchains (Rust: `cargo-llvm-cov` + `cargo-nextest`; Python: `pytest`).

### Notes

- `kiss test` still excludes doctests (see `VISION.md`); failing doctests can fail `cargo test` while `kiss test` passes.
- Requires a recent Rust toolchain (`edition = "2024"`).

## 0.4.10 — 2026-08-25

Previous crates.io release.
