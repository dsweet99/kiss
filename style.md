# kiss Style Guide

## Project Overview

**kiss** is a code-quality metrics tool for Python and Rust, written in Rust. LLM coder feedback alongside linters/test runners. **Primary consumer is the LLM** — output controls LLM behavior. Strict-by-default.

**Analysis types:** Count metrics, Graph analysis (fan-in/out, cycles, depth), Duplication (MinHash/LSH), Test references (static), Coverage gate (90% default).

## Design Philosophy

- **KISS is the ethos** — simplicity over sophistication in all choices
- **Component checks, not composites** — avoid derived metrics (God Class, LCOM, Cyclomatic) when components already catch the issue; derived metrics prove "finicky"
- **Empirical over arbitrary** — use `mimic` on respected codebases; max values, not percentiles
- **Redundancy aversion** — if two metrics overlap significantly, keep only one
- **Informational vs thresholded** — some metrics (fan-in/out) are useful for detection (orphans) but not for direct thresholds

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
| `counts.rs` / `rust_counts.rs` | Metrics and violations |
| `graph.rs` | Dependencies, cycles (Tarjan), module-level graph |
| `rule_defs.rs` | Self-documenting rule registry (auto-included in `kiss rules`) |
| `test_refs.rs` / `rust_test_refs.rs` | Test reference analysis |

**Dependency graph is module-level** (not code-unit level): matches import semantics, provides actionable refactoring units, keeps graph tractable.

## Output Format

**One line per item. No headers, footers, or explanatory text.**

- `VIOLATION:metric:file:line:name: message. suggestion.`
- `WARNING:test_coverage:file:line:name: message.`
- `NO VIOLATIONS` — final line when clean

## Rust Conventions

- **No file-level lint suppression** — `#![allow(clippy::...)]` is cheating; fix properly or allow at specific line with justification
- **Struct init syntax** — `Config { field: val, ..Default::default() }` not field reassignment
- **`Option<&T>` over `&Option<T>`** in function signatures
- **`writeln!` over `format!` + `push_str`** for string building
- Max 300 lines/file; extract to `tests/*.rs` when approaching limit
- Self-documenting names; no comments (code should be clear)
- `tests/fake_*` are test fixtures (intentionally bad) — use `--ignore fake_`
- `src/test_utils.rs` for shared library test helpers (`#[cfg(test)]` modules can't import from `tests/`)

## Key Algorithms

**Duplication (MinHash/LSH):** Normalize → 3-gram shingles → 100 MinHash → 20 bands → Jaccard ≥ 0.7. Skip functions <5 lines (filters builder patterns).

**Graph:** Tarjan's SCC for cycles. Module-level (file = module). Rust `mod foo;` declarations are dependency edges. External crates excluded.

**Test References:** Capture ALL path segments (`Foo::bar()` → both). Auto-mark trait impl methods. Traverse `#[cfg(test)]` inline modules.

**Metric Counting:**
- `imported_names_per_file`: counts each name (`from X import a, b` = 2), not statements
- `attributes_per_function`: excludes `#[doc]` (doc comments aren't real attributes)

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

`kiss [PATH]` analyze | `kiss rules` show rules | `kiss stats [--all]` summary | `kiss mimic --out FILE`

Options: `--lang`, `--config`, `--defaults`, `--ignore PREFIX`, `--warnings`
