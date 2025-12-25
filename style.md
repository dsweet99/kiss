# kiss Style Guide

## Project Overview

**kiss** is a code-quality metrics tool for Python and Rust, written in Rust. Designed as LLM coder feedback alongside linters/test runners.

**Analysis types:**
1. **Count metrics** — statements, arguments, indentation, methods, LCOM
2. **Graph analysis** — fan-in/out, cycles (Tarjan's SCC), transitive deps
3. **Duplication detection** — MinHash/LSH for near-duplicate code
4. **Test references** — static analysis of test coverage by name matching
5. **Coverage gate** — refuse analysis if test coverage < threshold (default 90%)

## Architecture

| Python Module | Rust Module | Purpose |
|---------------|-------------|---------|
| `parsing.rs` | `rust_parsing.rs` | AST parsing (tree-sitter / syn) |
| `units.rs` | `rust_units.rs` | Code unit extraction |
| `counts.rs` | `rust_counts.rs` | Metrics and violations |
| `graph.rs` | `rust_graph.rs` | Dependency graphs |
| `test_refs.rs` | `rust_test_refs.rs` | Test reference analysis |

Shared: `config.rs`, `discovery.rs`, `duplication.rs`, `stats.rs`

## Conventions

**Rust code:**
- Prefer `&Path` over `&PathBuf` in signatures; validate TOML before `as usize`
- Comments: only *why* explanations, algorithm docs, module-level docs

**Quality:**
- No workarounds to hide problems — write proper tests instead
- `kiss --lang rust` must pass cleanly with no violations or untested items
- Run `cargo clippy --fix` to address auto-fixable warnings

## Key Algorithms

### MinHash/LSH (duplication.rs)
1. Normalize: lowercase, collapse whitespace, digits → 'N'
2. Shingles: 3-grams of tokens
3. MinHash: 100 hash functions, deterministic coefficients
4. LSH: 20 bands for candidate pairs
5. Verify: Jaccard similarity ≥ 0.7

### LCOM (counts.rs, rust_counts.rs)
Lack of Cohesion of Methods: `pairs_not_sharing_fields / total_pairs`
- 0.0 = cohesive (methods share instance fields)
- 1.0 = no cohesion (methods use disjoint fields)
- God Class: methods > 20 AND LCOM > 50%

### Dependency Analysis (graph.rs)
- petgraph directed graph from imports/uses
- Tarjan's SCC for cycle detection
- DFS for transitive dependency count
- Instability = Ce / (Ca + Ce)

### Test Reference Analysis (rust_test_refs.rs)
- Collect definitions (functions, structs, methods) and references from test code
- Parse macro token streams (assert!, assert_eq!) via `ExprList` custom parser
- Capture all path segments: `Foo::bar()` → both `Foo` and `bar`
- Filter external crates (std, syn, tokio, etc.) to avoid false positives
- Trait impl methods auto-marked as tested if implementing type is referenced

## Configuration

TOML with language-specific sections:
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

Legacy `[thresholds]` section still supported.

## CLI

`kiss [PATH]` — analyze | `kiss stats` — summaries | `kiss mimic --out FILE` — generate config
Options: `--lang python|rust`, `--config FILE`

## Testing

284 tests (245 lib + 39 bin). Inline modules: `#[cfg(test)] mod tests`. Naming: `test_<function>_<scenario>`.

## Development

Rust 2024 edition. Run `cargo clippy --fix` for auto-fixable warnings.
