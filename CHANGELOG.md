# Changelog

## Unreleased

### Removed

- The watcher (`kiss test-watch`). `kiss test` always runs in the foreground; concurrent `kiss test` runs in one repository still wait for each other.
- The `[test] watch_settle_seconds` setting. Existing `.kissconfig` files that set it still load; the value is ignored.
- Coverage support: the `test_coverage` gate, its `kiss rules` entry, coverage report fields, and the coverage metrics tooling. `[test] test_coverage_threshold` and `test_coverage_scope` still load; their values are ignored.
- The Python runtime line-coverage cache. `kiss test` now selects Python tests the way it selects Rust tests: any change to a Python source reruns every Python test, and an unchanged tree reuses cached results (a cached failure reruns only with `--retry-bad`). Orphan detection is purely static.
- `cargo-llvm-cov` is no longer needed for Rust. `kiss test` runs Rust tests with `cargo nextest run` and reads pass, fail, timeout, and timing from nextest's output. Rust tests record no coverage, so any change to a Rust input reruns every Rust test.

### Changed

- `[test] num_jobs_llvm_cov` is now `num_jobs_nextest`; the old name still loads as an alias.
- Orphan detection judges Rust code by static references alone. It now counts names used inside macro calls and `use` paths, treats trait impl methods as reachable, and reaches a `mod.rs` module by its directory name.

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
