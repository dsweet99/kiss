# kiss Style Guide

## Project Overview

**kiss** is a code-quality metrics tool for Python and Rust, written in Rust. LLM coder feedback alongside linters/test runners. **Primary consumer is the LLM** — output controls LLM behavior. Strict-by-default.

**Analysis types:** Count metrics, Graph analysis (fan-in/out, cycles, LCOM), Duplication (MinHash/LSH), Test references (static), Coverage gate.

## Design Philosophy

- **KISS is the ethos** — simplicity over sophistication in all choices
- **Component checks, not composites** — avoid derived metrics (God Class, Instability, Cyclomatic) when components already catch the issue
- **Empirical over arbitrary** — use `mimic` on respected codebases; max values, not percentiles
- **Proactive guidance** — `rules` command primes LLM context before coding, not just reactive violations
- **Redundancy aversion** — if two metrics overlap significantly, keep only one

## Architecture

| Module | Purpose |
|--------|---------|
| `counts.rs` / `rust_counts.rs` | Metrics and violations |
| `graph.rs` | Dependencies, cycles (Tarjan), LCOM |
| `test_refs.rs` / `rust_test_refs.rs` | Test reference analysis |
| `config.rs`, `defaults.rs`, `duplication.rs`, `stats.rs` | Shared infrastructure |

## Output Format

**One line per item. No headers, footers, or explanatory text.**

- `VIOLATION:metric:file:line:name: message. suggestion.`
- `WARNING:test_coverage:file:line:name: message.`
- `NO VIOLATIONS` — final line when clean

Gate failure uses `VIOLATION:test_coverage:...` since it blocks analysis.

## Conventions

- Max 300 lines/file; refactor to meet it, don't lower threshold
- Prefer `&Path` over `&PathBuf`; self-documenting names over comments
- Single source of truth: defaults in `defaults.rs`
- **Eat your own dog food:** `kiss` must pass cleanly on its own codebase
- `tests/fake_*` are test fixtures (intentionally bad code) — use `--ignore fake_` to exclude

## Key Algorithms

**MinHash/LSH:** Normalize → 3-gram shingles → 100 MinHash → 20 bands → Jaccard ≥ 0.7

**LCOM:** `pairs_not_sharing_fields / total_pairs` (0.0 = cohesive, 1.0 = no cohesion)

**Graph:** Tarjan's SCC for cycles. External modules excluded from violations.

**Test References:**
- Capture ALL path segments: `Foo::bar()` → both `Foo` and `bar`
- Auto-mark trait impl methods when type is referenced
- Traverse `#[cfg(test)]` inline modules; filter external crates

## Violation Advice

| Metric | Good Advice | Avoid |
|--------|-------------|-------|
| `methods_per_type` | "Extract into separate types" | "Split impl blocks" |
| `fan_in` | "Ensure stable and well-tested" | "Split the module" |
| Duplication | "Extract fn, use traits/generics" | Just "shared function" |

## Configuration

Precedence: `defaults.rs` → `~/.kissconfig` → `./.kissconfig` → `--config`

**Reference codebases for mimic:** ripgrep, fd, bat (Rust); rich, click, attrs (Python)

```toml
[gate]
test_coverage_threshold = 90

[python]
statements_per_function = 40

[rust]
statements_per_function = 25
```

## CLI

```
kiss [PATH]              Analyze (gated by test coverage)
kiss rules               Show coding rules (LLM context priming)
kiss stats [PATH...]     Summary statistics
kiss mimic --out FILE    Generate config (uses max values from codebase)
```
Options: `--lang`, `--config`, `--defaults`, `--all` (bypass gate), `--ignore PREFIX` (skip paths with component starting with PREFIX), `--warnings` (show test coverage warnings)
