# kiss Style Guide

## Project Overview

**kiss** is a code-quality metrics tool for Python and Rust, written in Rust. LLM coder feedback alongside linters/test runners. **Primary consumer is the LLM** — output restrictions control LLM behavior. Strict-by-default.

**Analysis types:** Count metrics, Graph analysis (fan-in/out, cycles, orphans), Duplication (MinHash/LSH), Test references (static), Coverage gate (default 90%).

## Architecture

| Module | Purpose |
|--------|---------|
| `counts.rs` / `rust_counts.rs` | Metrics and violations |
| `graph.rs` | Dependency graphs, cycles (Tarjan), orphan detection |
| `test_refs.rs` / `rust_test_refs.rs` | Test reference analysis |
| `config.rs`, `defaults.rs`, `duplication.rs`, `stats.rs` | Shared infrastructure |

## Output Format

- `VIOLATION:metric:file:line:name: message. suggestion.`
- `UNCOVERED:test_coverage:file:line:name: Add test coverage for this code unit.`
- `NO VIOLATIONS` only when truly clean (no violations, duplicates, or untested items)
- Suggestions: specific, actionable, language-aware (Rust: traits/generics; Python: dataclasses)

## Conventions

- Prefer `&Path` over `&PathBuf`; max 500 lines/file
- Self-documenting: descriptive names and types over comments
- Single source of truth: defaults in `defaults.rs`
- `kiss --lang rust` must pass cleanly; measure before optimizing
- Tolerate duplication until 3+ instances justify extraction

## Key Algorithms

**MinHash/LSH:** Normalize → 3-gram shingles → 100 MinHash → 20 bands → Jaccard ≥ 0.7

**LCOM:** `pairs_not_sharing_fields / total_pairs` (0.0 = cohesive, 1.0 = no cohesion)

**Graph:** Tarjan's SCC for cycles; orphan = fan_in=0 AND fan_out=0 (excluding entry points)

**Test References edge cases:**
- Capture ALL path segments: `Foo::bar()` → both `Foo` and `bar`
- Auto-mark trait impl methods as "indirectly tested" when type is referenced
- Must traverse `#[cfg(test)]` inline modules; filter external crates

## Violation Advice

| Metric | Good Advice | Avoid |
|--------|-------------|-------|
| `methods_per_type` | "Extract into separate types" | "Split impl blocks" |
| `fan_in` | "Ensure stable and well-tested" | "Split the module" |
| Duplication | "Extract fn, use traits/generics" | Just "shared function" |

## Configuration

Precedence: `defaults.rs` → `~/.kissconfig` → `./.kissconfig`

Prefer empirical thresholds. Use `kiss mimic` on respected codebases:
- Rust: ripgrep, fd, bat
- Python: rich, click, attrs, httpx

```toml
[gate]
test_coverage_threshold = 90

[python]
statements_per_function = 30

[rust]
statements_per_function = 25
```

## CLI

```
kiss [PATH]              Analyze (gated by test coverage)
kiss rules               Show coding rules (LLM context priming)
kiss stats [PATH...]     Summary statistics
kiss mimic --out FILE    Generate config from respected codebase
```
Options: `--lang`, `--config`, `--all` (bypass gate)

## Testing

Inline: `#[cfg(test)] mod tests`. Naming: `test_<function>_<scenario>`. Rust 2024 edition.
