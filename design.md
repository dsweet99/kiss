# kiss Design Document

## Overview

`kiss` is a code-quality metrics tool that measures simple, actionable metrics for a codebase and reports violations and advice to stdout. It is designed to run alongside linters and test runners (e.g., `ruff`, `pytest`, `clippy`) as a feedback script for LLM coders.

**Supported languages:** Python, Rust

## Processing Pipeline

1. **Parse**: Traverse source files in a repo (recursive), parse into AST
2. **Extract**: Build "code units" from the AST (language-specific)
3. **Analyze**: Run four analysis passes on the code units (language-agnostic)
4. **Report**: Output violations and advice to stdout

## Code Units

A "code unit" is any of the following (mapped per language):

| Concept | Python | Rust |
|---------|--------|------|
| Function | `def` (module-level) | `fn` (module-level) |
| Method | `def` (in class) | `fn` (in impl block) |
| Type | `class` | `struct`, `enum` |
| Module | `.py` file | `mod` |
| File | `.py` file | `.rs` file |

Different metrics apply to different code unit types.

## File Filtering

- Respect `.gitignore`
- Process all `**/*.py` and `**/*.rs` files, including tests

## Language Support

### Architecture

```
┌─────────────────────────────────────┐
│     Language-Agnostic Core          │
│  - Metrics computation              │
│  - Graph analysis                   │
│  - Duplication detection            │
│  - Reporting & config               │
├─────────────────────────────────────┤
│     Language Adapters               │
│  ┌─────────────┬─────────────┐      │
│  │   Python    │    Rust     │      │
│  │             │   (syn)     │      │
│  └─────────────┴─────────────┘      │
└─────────────────────────────────────┘
```

Each adapter:
1. Parses source files into language-specific AST
2. Extracts code units and maps them to common types
3. Computes raw metric values
4. Passes data to the language-agnostic core

### Metric Portability

Most metrics transfer directly across languages:

| Metric | Python | Rust | Notes |
|--------|:------:|:----:|-------|
| Statements per function | ✓ | ✓ | |
| Lines per file | ✓ | ✓ | |
| Arguments per function | ✓ | ✓ | |
| Max indentation depth | ✓ | ✓ | |
| Branches per function | ✓ | ✓ | |
| Return statements | ✓ | ✓ | |
| Local variables | ✓ | ✓ | |
| Nested function depth | ✓ | ✓ | Rust: closures |
| Imports per file | ✓ | ✓ | `import` vs `use` |
| Methods per type | ✓ | ✓ | class vs impl |
| Types per file | ✓ | ✓ | |
| Graph analysis | ✓ | ✓ | Rust: easier (explicit imports) |
| Duplication | ✓ | ✓ | Text-based, language-agnostic |

### Language-Specific Adaptations

**Python:**
- Positional vs keyword-only arguments (important due to lack of static types)
- Test references via naming conventions and imports

**Rust:**
- No keyword args distinction (static types prevent mix-ups)
- Test detection via `#[test]` and `#[cfg(test)]` attributes
- Trait implementations as a form of coupling

## Configuration

Configurable thresholds are read from config files in this order (later overrides earlier):
1. `~/.kissconfig`
2. `./.kissconfig`

Use `--config <file>` to specify an alternate config file.

### Config File Format

The config file uses TOML with language-specific sections:

```toml
[gate]
test_coverage_threshold = 90  # percentage (0-100), default 90

[python]
statements_per_function = 50
positional_args = 3
keyword_only_args = 6
max_indentation = 4
branches_per_function = 10
local_variables = 10

[rust]
statements_per_function = 60
arguments = 5
max_indentation = 4
branches_per_function = 10
local_variables = 10

[shared]
lines_per_file = 500
types_per_file = 3
imports_per_file = 15
```

Language-specific metrics (e.g., `positional_args` for Python) only appear in their respective sections. Metrics that apply to both languages can go in `[shared]` or be duplicated per-language if different thresholds are desired.

## Command-Line Interface

### Commands

```
kiss [PATH]                              Analyze codebase, report violations
kiss stats [PATH...]                     Report summary statistics for all metrics
kiss mimic [PATH...]                     Generate config file (stdout) with thresholds
kiss mimic --out <file> [PATH...]        Generate config file (write/merge to file)
```

Note: `stats` and `mimic` accept multiple paths to combine statistics across codebases.

### Global Options

```
--config <file>       Use specified config instead of defaults
--lang <lang>         Only analyze specified language (python, rust)
--all                 Bypass test coverage gate, run all checks unconditionally
```

### Test Coverage Gate

By default, `kiss [PATH]` enforces a test coverage gate before reporting violations and refactoring suggestions.

**Rationale:** The primary consumer of `kiss` output is an LLM. If `kiss` suggests refactoring code that lacks tests, the LLM may make changes that break untested functionality. By gating on test coverage, we reduce the probability of risky refactoring.

**Default behavior:**
1. First, compute test reference coverage (% of functions/methods/classes referenced by test files)
2. If coverage < threshold (default: 90%), refuse to proceed
3. Report the coverage gap and exit

**Example output when gated:**
```
❌ Test coverage too low to safely suggest refactoring.

   Test reference coverage: 62% (threshold: 90%)
   Functions with test references: 187 / 302

   Add tests for untested code, then run kiss again.
   Or use --all to bypass this check and proceed anyway.
```

**Bypass:** Use `--all` to skip the gate and run all checks unconditionally.

**Threshold configuration:** The threshold is configurable in the config file:
```toml
[gate]
test_coverage_threshold = 90  # percentage, 0-100
```

**Commands affected:**
- `kiss [PATH]` — gated (default)
- `kiss stats` — not gated (informational only)
- `kiss mimic` — not gated (analyzing external code)

### `kiss stats`

Collects all metric values across the codebase and reports summary statistics, grouped by language:

```
=== Python (427 files) ===
| Metric                    | 50%ile | 90%ile | 95%ile | 99%ile | max |
|---------------------------|--------|--------|--------|--------|-----|
| Statements per function   |      8 |     25 |     40 |     78 | 230 |
| Positional args           |      2 |      3 |      4 |      5 |   8 |
| Keyword-only args         |      0 |      1 |      2 |      4 |   9 |
| ...                       |    ... |    ... |    ... |    ... | ... |

=== Rust (183 files) ===
| Metric                    | 50%ile | 90%ile | 95%ile | 99%ile | max |
|---------------------------|--------|--------|--------|--------|-----|
| Statements per function   |     12 |     30 |     45 |     85 | 310 |
| Arguments                 |      2 |      4 |      5 |      6 |  11 |
| ...                       |    ... |    ... |    ... |    ... | ... |
```

**Use case:** Understand the distribution of your codebase before setting thresholds.

### `kiss mimic`

Analyzes one or more "respected" codebases and generates a config file with thresholds set to the 99th percentile values.

**Workflow:**
1. Find codebases you respect (e.g., well-maintained open source projects)
2. Run `kiss mimic /path/to/good/code1 /path/to/good/code2 --out .kissconfig`
3. Use the generated config to analyze and refactor your own codebase

**Rationale:** Instead of guessing thresholds, derive them empirically from code you trust. The 99th percentile means "allow everything those codebases allow, flag anything worse."

**Multiple paths:** Combining multiple codebases gives a broader sample and more robust percentiles.

**Merge mode:** When writing to a file with `--out`:
- Reads the existing config file if present
- Updates only the language sections for languages found in the analyzed paths
- Preserves other language sections unchanged

This allows incremental updates:
```bash
# First, mimic a Python codebase
kiss mimic /path/to/good/python --out .kissconfig

# Later, add Rust thresholds from a Rust codebase
kiss mimic /path/to/good/rust --out .kissconfig
# → Python section is preserved, Rust section is added/updated
```

**Language filter:** Use `--lang` to analyze only specific languages:
```bash
kiss mimic --lang rust /path/to/mixed/code --out .kissconfig
# → Only analyzes .rs files, only updates [rust] section
```

## Output Format

Plain text, human-readable but primarily designed for LLM consumption. Each violation should include:
- File name
- Line number(s)
- Metric name and value
- Problem description
- Brief, clear suggestion for how to fix (based on software engineering best practices)

## Analysis Types

### 1. Counts

Measure per-code-unit metrics:

| Metric | Unit | Typical Threshold |
|--------|------|-------------------|
| Statements per function | function | > 50 |
| Methods per class | class | > 20 |
| Lines per file | file | > 500 |
| Positional arguments per function | function | > 3 |
| Keyword-only arguments per function | function | > 6 |
| Total arguments per function | function | > 7 |
| Max indentation depth | function | > 4 |
| Classes per file | file | > 3 |
| Nested function depth | function | > 2 |
| Return statements per function | function | > 5 |
| Branches (if/elif/else) per function | function | > 10 |
| Local variables per function | function | > 10 |
| Import count per file | file | > 15 |

### 2. Graph

Build a directed graph showing how code units couple to (call/use) other code units.

#### Core Metrics (directly computed)

| Metric | Scope | Typical Threshold | What It Measures |
|--------|-------|-------------------|------------------|
| Fan-in | function/class | > 20 | How many units call/use this unit |
| Fan-out | function/class | > 10 | How many units this unit calls/uses |
| Afferent coupling (Ca) | module | > 15 | Incoming dependencies from other modules |
| Efferent coupling (Ce) | module | > 10 | Outgoing dependencies to other modules |
| Transitive dependencies | function/class | > 30 | All dependencies (direct + indirect) |
| Strongly connected components | graph-wide | > 0 (any cycle) | Circular dependency clusters |
| LCOM (Lack of Cohesion) | class | > 0.5 | How well methods share fields (0=cohesive, 1=no cohesion) |

#### Derived Metrics (computed from core)

| Metric | Formula | Threshold | What It Means |
|--------|---------|-----------|---------------|
| Instability | Ce / (Ca + Ce) | report only | 0 = stable, 1 = unstable (not inherently bad) |
| God Class indicator | methods > 20 AND LCOM > 50% | any trigger | Class doing too much |

#### Metrics Intentionally Omitted

To avoid redundancy:

| Metric | Why Omitted |
|--------|-------------|
| WMC (Weighted Methods per Class) | = methods × avg complexity; redundant |
| DIT / NOC (inheritance depth/children) | Less relevant for Python/Rust (composition over inheritance) |
| Centrality / PageRank | Sophisticated fan-in; overkill for now |
| Feature Envy | Requires detailed field tracking; complex |

**Note:** Cyclomatic Complexity is computed separately from Branches per function. Branches counts only if/elif/else statements, while Cyclomatic Complexity includes loops and boolean operators (&& / ||). The suggestions are differentiated accordingly.

### 3. Duplication

Detect duplicated code across code units and identify DRY opportunities.

Adapt the LSH/MinHash algorithm from `../dryer` for this purpose.

### 4. Test References

Identify code units that may lack test coverage using static analysis (no code execution required).

**Approach:**
1. Parse source files → list all functions, methods, and classes
2. Parse test files → extract all names referenced (calls, imports, instantiations)
3. Report source code units with no test references as "possibly untested"

**Why this works for `kiss`:** The other metrics encourage simpler code with fewer branches. When developers follow this guidance, code units become more linear, and "test references" correlates more strongly with actual test coverage. Simple functions with few branches are either tested or not — there's less room for partial coverage.

**Limitations (be honest in output):**
- This is "test references," not true coverage
- Cannot detect branch coverage
- Dynamic calls and indirect references are invisible
- A referenced function may still have untested edge cases

#### Test File Detection

How `kiss` identifies test files vs source files:

**Python:**
- Files matching `test_*.py` or `*_test.py`
- Files inside directories named `tests/` or `test/`
- Files containing `pytest` or `unittest` imports (fallback heuristic)

**Rust:**
- Inline `#[cfg(test)]` modules (within source files)
- Files inside `tests/` directory (integration tests)
- Files matching `*_test.rs` or `test_*.rs`

#### Implementation Edge Cases

**1. Path Segment Collection**

When collecting references from test code, collect ALL segments of a path, not just the final one.

Example: `MyStruct::new()` should mark both `MyStruct` and `new` as referenced.

Without this, types referenced via associated functions appear "untested."

**Nuance:** Only collect segments that resolve to local definitions. Ignore external crate paths like `std::collections::HashMap` — we don't need to test the standard library.

**2. Trait Implementations**

Methods defined in trait impl blocks (e.g., `impl Display for MyType`) are called indirectly:
- Tests call `println!("{}", my_type)`, not `my_type.fmt()`
- The `fmt` method appears "untested" even when `MyType` is heavily used

**Solution:** If a type is referenced by tests, auto-mark its trait impl methods as "indirectly referenced."

Report separately:
```
Directly tested: 45 functions
Indirectly tested (via type usage): 12 trait impl methods
Possibly untested: 8 functions
```

**3. Inline Test Module Traversal (Rust)**

Rust's idiomatic pattern is inline test modules:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_something() { ... }
}
```

The reference collector MUST traverse these modules. If `#[cfg(test)]` modules aren't being found, inline tests won't be counted, causing widespread false positives.

**Verification:** Log which modules are traversed. Ensure `#[cfg(test)]` attribute matching works correctly.
- Files matching `*_test.rs` or `test_*.rs`

Source files are everything else matching the language extension.

---

## Appendix: Metric Definitions

### Graph Metrics

#### Fan-in / Fan-out

- **Fan-in**: How many other code units *call/use* this unit (incoming edges)
- **Fan-out**: How many other code units this unit *calls/uses* (outgoing edges)

**What it tells you:**
- High fan-in → heavily depended upon (core utility, hard to change safely)
- High fan-out → depends on many things (fragile, likely doing too much)

**Example:** A `utils.py` helper function might have fan-in=50 (everyone uses it). A "god function" might have fan-out=30 (it calls everything).

#### Strongly Connected Components (Cycles)

A group of code units where each unit can reach every other unit through the dependency graph. Essentially: circular dependencies.

**What it tells you:**
- Cycles make code hard to understand, test, and refactor
- You can't change A without considering B, C, D... which all depend on each other

**Example:** `A → B → C → A` is a cycle. To understand any of them, you must understand all of them.

**Note:** Cyclomatic Complexity is a related concept (counting decision paths through a function), but we capture this via "Branches per function" in Count metrics to avoid redundancy.

#### Afferent / Efferent Coupling

These measure coupling at the module/package level:

- **Afferent (Ca)**: Number of *outside* modules that depend on this module (incoming)
- **Efferent (Ce)**: Number of *outside* modules this module depends on (outgoing)

**Derived metric - Instability**: `I = Ce / (Ca + Ce)`
- I = 0 → maximally stable (everyone depends on you, you depend on nobody)
- I = 1 → maximally unstable (you depend on everyone, nobody depends on you)

**What it tells you:**
- Stable modules (low I) should be abstract/interfaces
- Unstable modules (high I) should be concrete implementations
- Violations of this principle indicate architectural issues

#### Transitive Dependencies

The total number of code units this unit depends on, directly or indirectly.

**How it differs from fan-out:**
- Fan-out = direct dependencies only
- Transitive = all reachable dependencies in the graph

**What it tells you:**
- High transitive deps = fragile; many things can break you
- A change deep in your dependency chain can ripple up to you
- Useful for identifying "high risk" code units

**Example:** Function A calls B and C. B calls D, E, F. C calls G.
- Fan-out of A = 2 (B, C)
- Transitive deps of A = 6 (B, C, D, E, F, G)

#### LCOM (Lack of Cohesion of Methods)

Measures whether methods in a class use the same instance fields.

**Intuition:** If methods share fields, they're working on the same data → cohesive. If methods use disjoint sets of fields, the class might be doing unrelated things → should split.

**Simple formula (LCOM1):**
- Count pairs of methods that share no fields
- Count pairs of methods that share at least one field
- LCOM = max(0, pairs_sharing_none - pairs_sharing_some)

**What it tells you:**
- LCOM = 0 → perfectly cohesive
- High LCOM → class has unrelated responsibilities, consider splitting

**Example:** A class with methods `get_name()`, `set_name()` (use `self.name`) and `calculate_tax()`, `format_invoice()` (use `self.items`, `self.total`) has low cohesion — it's mixing identity and billing.

#### God Class Indicator

A derived metric combining several signals:

**Triggers when a class has:**
- High method count (> threshold)
- High fan-out (depends on many other units)
- Low cohesion (high LCOM)
- Optionally: high lines of code

**What it tells you:**
- The class has accumulated too many responsibilities
- It's a maintenance burden and testing nightmare
- Should be refactored into multiple focused classes

### Count Metrics

#### Statements per Function

**High value means:**
- Function is doing too much
- Hard to understand, test, and maintain
- Should be broken into smaller, focused functions

#### Methods per Class

**High value means:**
- Class has too many responsibilities (violates Single Responsibility Principle)
- Consider splitting into multiple classes
- May indicate a "god class" anti-pattern

#### Lines per File

**High value means:**
- Module is doing too much
- Hard to navigate and understand
- Should be split into multiple modules

#### Arguments per Function

Arguments are counted separately by type because they carry different risks.

**Positional arguments** (e.g., `def foo(a, b, c)`):
- High count indicates function does too much
- **Order-dependent**: easy to mix up arguments, especially during refactoring
- Particularly dangerous in Python which lacks static type checking
- Threshold: keep to 3 or fewer

**Keyword-only arguments** (e.g., `def foo(a, *, name=None, count=0)`):
- Still indicates complexity if excessive
- **Order-independent**: caller must name them, preventing mix-ups
- Self-documenting at call site
- Refactoring-safe (can reorder parameters)
- Threshold: more permissive (6 or fewer)

**Actionable suggestion:** When a function has many positional arguments, `kiss` should suggest converting some to keyword-only:

> "Function `process_data` has 5 positional arguments. Consider using keyword-only arguments (`*`) after the first 2-3 to prevent argument order mistakes."

**Total arguments** should still be bounded (7 or fewer) — even named arguments add cognitive load.

#### Max Indentation Depth

**High value means:**
- Deeply nested logic is hard to follow
- Consider early returns, guard clauses, or extracting helper functions
- Often indicates too many nested conditionals or loops

#### Classes per File

**High value means:**
- File is doing too much
- Each class should typically have its own file (or closely related classes grouped)
- Harder to find and navigate code

#### Nested Function Depth

Functions defined inside functions, inside functions...

**High value means:**
- Hard to test inner functions in isolation
- Closure complexity (inner functions capture outer scope variables)
- Often indicates a function doing too much that should be refactored into a class or module

#### Return Statements per Function

**High value means:**
- Multiple exit points → harder to reason about what gets returned when
- Often indicates the function handles too many cases
- Debugging is harder (which return was hit?)

**Nuance:** Early returns for guard clauses (e.g., `if not x: return None`) are fine and can improve readability. It's scattered returns deep in logic that hurt.

#### Branches (if/elif/else) per Function

**High value means:**
- High cyclomatic complexity
- More test cases needed for coverage
- Often a sign of missing polymorphism or strategy pattern
- The function is making too many decisions

**Example concern:** 10+ branches often means the function is a "dispatcher" that should be a dictionary lookup, class hierarchy, or separate functions.

#### Local Variables per Function

**High value means:**
- Function is juggling too much state
- Harder to hold in your head
- Often indicates the function does multiple things that should be separated
- Higher chance of variable name collisions or shadowing

**Rule of thumb:** 7±2 is the cognitive limit for most people. Beyond ~10 local variables, the function is likely doing too much.

#### Import Count per File

**High value means:**
- High efferent coupling (depends on many things)
- More likely to break when dependencies change
- Often indicates the module has too many responsibilities
- Slower to load, more dependency conflicts

**Nuance:** Standard library imports are less concerning than third-party or internal imports.

### Test References

#### What It Measures

Static analysis of whether code units are referenced by test files. This is a fast approximation of test coverage that requires no code execution.

**How it works:**
1. Scan source files for all defined functions, methods, and classes
2. Scan test files for all referenced names (function calls, class instantiations, imports)
3. Match references to definitions
4. Report unmatched source definitions as "possibly untested"

**Why it's useful despite limitations:**

True test coverage requires execution because Python is dynamic — you can't statically determine which branch of an `if` statement runs, or which class's method is called via polymorphism.

However, `kiss` encourages simpler code:
- Fewer branches per function
- Smaller functions
- Less nesting

When code is simple and linear, "is this function called by a test?" becomes a strong proxy for "is this function tested?" A 5-line function with no branches is either tested or not — there's no partial coverage to miss.

**What to report:**
- Functions/methods/classes with zero test references
- Optionally: low reference count (tested by only one test file)

**Honest labeling:** Output should say "possibly untested" or "no test references found," not "untested" — static analysis cannot prove the negative.
