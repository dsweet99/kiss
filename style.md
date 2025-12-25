# kiss Style Guide

## Project Overview

**kiss** is a code-quality metrics tool for Python and Rust, written in Rust. Designed as LLM coder feedback alongside linters/test runners.

**Analysis types:**
1. **Count metrics** — statements, arguments, indentation, methods, LCOM
2. **Graph analysis** — fan-in/out, cycles (Tarjan's SCC), transitive deps, orphan detection
3. **Duplication detection** — MinHash/LSH for near-duplicate code
4. **Test references** — static analysis of test coverage by name matching
5. **Coverage gate** — refuse analysis if test coverage < threshold (default 90%)

## Architecture

| Python Module | Rust Module | Purpose |
|---------------|-------------|---------|
| `parsing.rs` | `rust_parsing.rs` | AST parsing (tree-sitter / syn) |
| `counts.rs` | `rust_counts.rs` | Metrics and violations |
| `graph.rs` | — | Dependency graphs (shared) |
| `test_refs.rs` | `rust_test_refs.rs` | Test reference analysis |

Shared: `config.rs`, `discovery.rs`, `duplication.rs`, `stats.rs`, `cli_output.rs`

## Output Format

Single-line violations: `VIOLATION:file:line: value metric. message. suggestion.`
- `NO VIOLATIONS` only when truly clean (no metric violations AND no duplicates)
- Suggestions must be specific and actionable, not vague ("use guard clauses" not "restructure")
- Language-aware: Rust advice mentions traits/generics; Python mentions dataclasses

## Conventions

**Code:**
- Prefer `&Path` over `&PathBuf` in signatures
- Max 500 lines per file; inline small helpers if they trigger duplication detection
- Comments: only *why* explanations, algorithm docs, module-level docs

**Quality:**
- `kiss --lang rust` must pass cleanly with no violations or untested items
- No workarounds — write proper tests instead
- Run `cargo clippy --fix` for auto-fixable warnings

## Key Algorithms

### MinHash/LSH (duplication.rs)
Normalize → 3-gram shingles → 100 MinHash functions → 20 LSH bands → Jaccard ≥ 0.7

### LCOM (counts.rs, rust_counts.rs)
`pairs_not_sharing_fields / total_pairs` (0.0 = cohesive, 1.0 = no cohesion)
God Class: methods > 20 AND LCOM > 50%

### Dependency Analysis (graph.rs)
- Tarjan's SCC for cycle detection
- Orphan detection: fan_in=0 AND fan_out=0 (excluding main/lib/test_*)
- High fan-in is acceptable for utility modules — advise stability, not splitting

### Test Reference Analysis (rust_test_refs.rs)
- Parse macro tokens (assert!, assert_eq!) via `ExprList` custom parser
- Capture all path segments: `Foo::bar()` → both `Foo` and `bar`
- Filter external crates (std, syn, tokio, etc.)

## Violation Advice Guidelines

| Metric | Good Advice | Avoid |
|--------|-------------|-------|
| `methods_per_type` (Rust) | "Extract into separate types" | "Split into multiple impl blocks" |
| `returns_per_function` | "Use guard returns at top, single main return" | "Restructure logic" |
| `transitive_deps` | "Introduce interfaces, use DI, split module" | "Reduce coupling" |
| `fan_in` | "Ensure stable and well-tested" | "Split the module" |
| `cyclomatic_complexity` | "Reduce if/for/while/and/or expressions" | Same as branches |
| Duplication (Rust) | "Extract function, or use traits/generics" | Just "shared function" |
| Untested code | "Add tests or remove if dead code" | (no suggestion) |

## Configuration

```toml
[shared]
lines_per_file = 500

[python]
statements_per_function = 50
positional_args = 3

[rust]
statements_per_function = 60
arguments = 5
```

## CLI

`kiss [PATH]` — analyze | `kiss stats` — summaries | `kiss mimic --out FILE` — generate config
Options: `--lang python|rust`, `--config FILE`, `--all` (bypass coverage gate)

## Testing

Inline modules: `#[cfg(test)] mod tests`. Naming: `test_<function>_<scenario>`.
Rust 2024 edition.
