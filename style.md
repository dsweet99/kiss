# kiss Style Guide

## Project Overview

**kiss** (`kiss-ai` on crates.io) is a code-quality metrics tool for Python and Rust, written in Rust (edition 2024, stable since Rust 1.85). LLM coder feedback alongside linters/test runners. **Primary consumer is the LLM** — output controls LLM behavior. Strict-by-default. Self-hosting: `kiss check .` must pass.

**Analysis types:** Count metrics, Graph analysis (fan-in/out, cycles, depth), Duplication (MinHash/LSH), Test references (static), Coverage gate (90% default).

## Design Philosophy

- **KISS is the ethos** — simplicity over sophistication; local macros beat parameterized shared ones
- **Component checks, not composites** — avoid derived metrics (God Class, LCOM, Cyclomatic); components catch the issue
- **Empirical over arbitrary** — use `mimic` on respected codebases; max values, not percentiles
- **Distinguish coupling from API surface** — count internal `use`, not `pub use` re-exports; exempt module definition files
- **Semantic consistency** — equivalent metrics should measure the same thing across Python and Rust

## Global Metrics (LLM Peripheral Vision)

LLMs work locally but need global awareness. **Pattern:** Local action → Global consequence → LLM blindspot → Actionable fix.

| Metric | Measures | Local trigger |
|--------|----------|---------------|
| `max_depth` | Longest dependency chain | Adding an import |
| `cycle_count` | Circular dependencies | Import that closes a loop |
| `orphan_count` | Dead modules | Refactoring that removes last reference |

**Delta reporting:** "max_depth increased 5→6" beats "max_depth is 6".

## Architecture

| Module | Purpose |
|--------|---------|
| `py_metrics.rs` / `rust_fn_metrics.rs` | Metric computation (tree-sitter / syn) |
| `counts.rs` / `rust_counts.rs` | Violation checking against thresholds |
| `graph.rs` | Dependencies, cycles (Tarjan), module-level graph |
| `config.rs` / `gate_config.rs` | Thresholds config; gate config (coverage, similarity) |

**Dependency graph is module-level** (file = module): matches import semantics, actionable refactoring units.

## Output Format

**One line per item. No headers, footers, or explanatory text.**

- `VIOLATION:metric:file:line:name: message. suggestion.`
- `WARNING:test_coverage:file:line:name: message.`
- `NO VIOLATIONS` — final line when clean

## Rust Conventions

- **No file-level lint suppression** — fix properly or allow at specific line with justification comment
- **Struct init syntax** — `Config { field: val, ..Default::default() }` not field reassignment
- **`Option<&T>` over `&Option<T>`** in function signatures; `writeln!` over `format!` + `push_str`
- Max 300 lines/file; extract to `tests/*.rs` or submodules when approaching limit
- `NOT_APPLICABLE` constant for cross-language config fields that don't apply (e.g., `statements_per_try_block` for Rust)
- Neutral internal names (`annotations_per_function`) with language-specific external keys (`decorators_per_function`, `attributes_per_function`)
- `src/test_utils.rs` for shared test helpers; `tests/fake_*` are intentionally-bad fixtures (`.kissignore`)

## Key Algorithms

**Duplication (MinHash/LSH):** Normalize → 3-gram shingles → 100 MinHash → 20 bands → Jaccard ≥ 0.7. Skip functions <5 lines (filters builder patterns).

**Graph:** Tarjan's SCC for cycles. Module-level (file = module). Rust `mod foo;` declarations are dependency edges. External crates excluded. **Import scope:** Both Python and Rust extract imports from ALL scopes (including function bodies, closures, impl blocks) for consistent dependency analysis.

**Test References:** Capture ALL path segments (`Foo::bar()` → both). Auto-mark trait impl methods. Traverse `#[cfg(test)]` inline modules.

**Metric Counting:**
- `statements_per_file`: counts statements **inside function/method bodies only** — not imports, class/function signatures
- `imported_names_per_file`: counts **unique** names (`import torch` twice = 1); Rust counts only non-`pub use`
- Module definition files (`__init__.py`, `lib.rs`, `mod.rs`) exempt from import limits — they aggregate by design
- `attributes_per_function`: excludes `#[doc]` (doc comments aren't real attributes)
- `branches_per_function`: Python counts `if/elif/case_clause`; Rust counts only `if` (match arms are exhaustive, not optional branches)

## Violation Advice

| Metric | Good Advice | Avoid |
|--------|-------------|-------|
| `methods_per_type` | "Extract into separate types" | "Split impl blocks" |
| `fan_in` (informational) | "Ensure stable and well-tested" | "Split the module" |
| Duplication | "Extract fn, use traits/generics" | Just "shared function" |

## Configuration

Precedence: `defaults.rs` → `~/.kissconfig` → `./.kissconfig` → `--config`

**No backwards compatibility** — new software; rename/remove freely. Descriptive metric names over terse.

**Reference codebases for mimic:** ripgrep, fd, bat (Rust); rich, click, attrs (Python)

## CLI

`kiss check [PATH]` | `kiss rules` | `kiss stats [--all]` | `kiss mimic --out FILE` | `kiss clamp` | `kiss config`

Options: `--lang`, `--config`, `--defaults`, `--ignore PREFIX`, `--all`
