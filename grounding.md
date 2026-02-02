## `kiss` grounding: design + intent

`kiss` is a **code-quality metrics tool for Python and Rust** whose primary consumer is an **LLM coding agent**, not a human. It exists to turn “this code feels too complex/coupled/duplicated/undertested” into **simple, actionable, machine-readable feedback** that nudges an agent toward smaller units, clearer boundaries, less duplication, and better test coverage.

### What problem `kiss` is trying to solve

- **LLMs have strong local focus**: they can produce a correct patch in the file they’re editing, while accidentally making the *whole codebase* harder to maintain (more coupling, deeper dependency chains, larger units).
- Traditional linters often target stylistic or language-specific issues; `kiss` targets **structural maintainability** and **global consequences**.
- The goal is not “perfect code”, it’s **keeping code easy for agents (and humans) to change** by preventing complexity growth and spotlighting outliers.

### Performance is a first-class goal (inner-loop feedback)

`kiss` is intended to run **in the inner LLM coding loop**, so it aims to be **fast enough to run frequently** (alongside tests/linters) without feeling “expensive”.

Current speed-oriented choices include:

- **Parallelism**: file-level work is parallelized (e.g., parsing/analysis over many files).
- **Module-level dependency graph**: coupling is measured at the file/module boundary (file = module), which is far cheaper and more actionable than building full symbol graphs.
- **Fast cycle detection**: cycles are found via SCC (Tarjan) on the module graph.
- **Approximate duplication detection**: duplication uses **MinHash + LSH** to cheaply find “likely similar” code blocks without comparing every pair.
- **Static test-reference coverage**: “coverage” is a fast static reference check (names referenced by tests), not runtime coverage instrumentation.
- **Early exit gates**: `kiss check` can stop early on gate failure unless explicitly bypassed.

### Core philosophy

- **KISS is the ethos**: prefer straightforward refactors and simple abstractions over clever designs.
- **Component checks, not composite scores**: avoid derived “mega-metrics” (e.g., God Class / LCOM). Instead, measure the components that cause them (size, depth, branching, coupling, duplication).
- **Empirical over arbitrary**: thresholds can be derived from real codebases you respect via `kiss mimic`, rather than invented.
- **Cross-language consistency**: “the same metric name should mean the same thing” across Python and Rust wherever possible.

### The contract: output is for tooling

`kiss check` emits **one line per item** with stable prefixes so an agent can parse it reliably:

- `VIOLATION:<metric>:<file>:<line>:<name>: <message> <suggestion>`
- `GATE_FAILED:test_coverage: ...` (a hard stop unless you bypass)
- `NO VIOLATIONS` (final success sentinel)

The intent is that a runner/agent can:

- stream the output,
- turn each violation into a to-do,
- and iterate until `NO VIOLATIONS`.

### How `kiss check` works (pipeline)

`kiss check` runs a pipeline over discovered source files (optionally filtered by language) and emits violations from several analysis types:

- **Discovery**: find Python/Rust source files with ignore support.
- **Parsing**:
  - Python via tree-sitter
  - Rust via `syn`
- **Local complexity metrics (counts)**:
  - per-function: statements, arguments, branches, locals, returns, nesting depth, etc.
  - per-file: statements (inside bodies), number of functions/types, imported names, etc.
- **Dependency graph analysis (module-level)**:
  - cycles (SCC/Tarjan), dependency depth, transitive dependency counts, orphan modules.
- **Duplication detection (approximate)**:
  - MinHash/LSH-based similarity to flag copy/paste blocks and encourage extraction.
- **Test-reference coverage (static)**:
  - treat “is referenced by tests” as a gateable property to keep changes grounded in tests.

### “Universe vs focus” (how `check` scopes work)

`kiss check` supports a “global analysis, local reporting” workflow:

- The **first path** is the **universe**: everything used to compute graphs/coverage and find context.
- Additional paths are **focus paths**: only violations from these files are reported.

This matches how an agent works: compute global consequences, but report only what’s relevant to the current edit.

### Gates: fail fast unless bypassed

Some feedback is treated as a **gate** (not just “more violations”):

- By default, `kiss check` can **stop early** on insufficient test-reference coverage and print `GATE_FAILED:test_coverage: ...`.
- `--all` bypasses the gate so you can explore and see all violations anyway.

The intention is to make “working without tests” a deliberate choice, not an accident.

### Configuration model (tight defaults, easy adoption)

`kiss` is meant to be **strict-by-default** but adoptable on existing repos:

- **Config precedence**: built-in defaults → `~/.kissconfig` → `./.kissconfig` → `--config`.
- `kiss clamp`: generate a repo-local `.kissconfig` that matches today’s code, preventing further complexity growth.
- `kiss mimic PATH --out FILE`: infer thresholds from another “good” codebase to encode a taste/standard.

This supports a gradual workflow: clamp now, ratchet down later.

### Why the dependency graph is module-level

`kiss` models dependencies at the **file/module level** because that is:

- aligned with Python/Rust import semantics,
- a natural unit of refactoring (move/split modules),
- and easier for an agent to act on than fine-grained symbol graphs.

### Intended workflow (how an agent should use it)

- Put `kiss rules` output in the agent’s context to bias generation toward compliant code.
- Run `kiss check` as part of the loop with tests and formatters/linters.
- When onboarding an existing codebase, run `kiss clamp`, then gradually tighten.
- When you want a “style target”, run `kiss mimic` on a codebase you consider simple/clean.

