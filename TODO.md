# kiss Implementation Tasks

High-level tasks to satisfy the design in `design.md`.

## Pipeline ✅

### Python
- [x] **File Traversal** — Walk directories to find `*.py` files, respecting `.gitignore`
- [x] **Parse Python AST** — Parse Python files into an AST representation (tree-sitter)
- [x] **Extract Code Units** — Build code units (functions, methods, classes, modules, files) from the AST

### Rust
- [x] **File Traversal** — Extend discovery to find `*.rs` files
- [x] **Parse Rust AST** — Use `syn` crate to parse Rust files
- [x] **Extract Code Units** — Map Rust constructs to common code unit types

## Analysis Types

### 1. Counts ✅
- [x] Statements per function
- [x] Methods per class/type
- [x] Lines per file
- [x] Positional arguments per function (Python)
- [x] Keyword-only arguments per function (Python)
- [x] Total arguments per function
- [x] Max indentation depth
- [x] Classes/types per file
- [x] Nested function depth
- [x] Return statements per function
- [x] Branches per function
- [x] Local variables per function
- [x] Import count per file

### 2. Graph ✅
- [x] Build dependency graph from imports/use statements
- [x] Fan-in (threshold > 20)
- [x] Fan-out (threshold > 10)
- [x] Strongly connected components (cycle detection)
- [x] Instability (Ce / (Ca + Ce)) — report only
- [x] Transitive dependencies (threshold > 30)

### 3. Class Cohesion ✅
- [x] LCOM (Lack of Cohesion of Methods) (threshold > 50%)
- [x] God Class indicator (methods > 20 AND LCOM > 50%)

### 4. Duplication ✅
- [x] MinHash/LSH algorithm for near-duplicate detection
- [x] Cluster duplicates for reporting
- [x] Python support (text-based, applies to any language)

### 5. Test References ✅
- [x] Python: detect `test_*.py`, `*_test.py`, `tests/` directory
- [x] Python: detect files with `pytest` or `unittest` imports (fallback heuristic)
- [x] Rust: detect `#[test]`, `#[cfg(test)]`, `tests/` directory
- [x] Report code units with no test references
- [x] **Path Segment Collection** — Capture ALL path segments (e.g., `MyStruct::new()` → both `MyStruct` and `new`)
- [x] **Trait Impl Methods** — Auto-mark trait impl methods as covered if type is referenced
- [x] **Macro Token Parsing** — Extract references from inside `assert!`, `assert_eq!`, etc.

## CLI ✅

- [x] `kiss [PATH]` — Analyze codebase, report violations
- [x] `kiss stats [PATH...]` — Report summary statistics grouped by language
- [x] `kiss mimic [PATH...]` — Generate config with 99th percentile thresholds
- [x] `kiss mimic --out <file>` — Write/merge config to file (preserves existing sections)
- [x] `--config <file>` — Use specified config file
- [x] `--lang <lang>` — Filter to Python or Rust only

## Configuration ✅

- [x] Load from `~/.kissconfig` and `./.kissconfig`
- [x] TOML format with `[python]`, `[rust]`, `[shared]` sections
- [x] Language isolation — Python uses `[python]`, Rust uses `[rust]`
- [x] Legacy `[thresholds]` section for backwards compatibility

## Output Format ✅

- [x] File name and line number
- [x] Metric name and value
- [x] Problem description
- [x] Suggestion for how to fix

## Test Coverage ✅

- [x] Core modules (lib, config, stats, graph, duplication)
- [x] Rust modules (parsing, units, counts, graph, test_refs)
- [x] CLI integration tests (10 tests verifying binary behavior)
- [x] Empty file edge case tests (Python and Rust)
- [x] 298+ tests total (249 lib + 39 bin + 10 integration)

---

## Missing Functionality (per design.md) ✅

- [x] **`kiss rules` command** — Output compact list of coding rules for LLM context priming (design.md lines 153-199). Should load config and emit imperative rules like "Keep functions ≤ 50 statements". Supports `--lang` filter.

---

## Code Quality Issues ✅

- [x] **Move defaults to `defaults.rs`** — `returns_per_function` (5) and `nested_function_depth` (2) are hardcoded in `config.rs` instead of `defaults.rs`. Should follow "single source of truth" per style.md.
- [x] **Add missing metrics to default config TOML** — `returns_per_function` and `nested_function_depth` are not included in `default_config_toml()` output.

---

## Future Enhancements

Optional improvements not required by design.md:

- [ ] **Language Adapter Trait** — Abstract language-specific parsing behind a common trait
- [x] **Rust Duplication Detection** — Extend duplication to Rust files
- [x] **Stringly-typed Cleanup** — Replace string `kind` fields with enums
- [x] **DRY Violation Building** — Factor out common violation-building code
- [x] **Add `#[must_use]`** — On key public functions
- [x] **Instability Reporting** — Report instability in main analysis output
