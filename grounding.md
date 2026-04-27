# `kiss` grounding

This document is **implementation-agnostic**. It is a reference for the
*intention*, *objectives*, and *constraints* that must remain true even
if algorithms, parsers, parallelism strategy, or internal structure
change.

`kiss` has two related surfaces, both grounded here:

- **Measurement and feedback**: `check`, `stats`, `shrink`,
  `show-tests`, `mimic`, `clamp`, `init`, `rules`, `config`, `dry`,
  `viz`. These analyze a codebase and report on it.
- **Semantic refactoring**: `mv`. This proposes and applies
  meaning-preserving rename/move edits to Python and Rust symbols.

The two surfaces share the same overall ethos (machine-readable output,
strict-by-default, fast enough for the inner loop, deterministic) but
have distinct correctness contracts; their constraints are listed
separately below.

---

## Intention

`kiss` exists to give an **LLM coding agent** fast, machine-readable
help in two complementary modes:

- *Diagnose*: structural-maintainability feedback so the agent's local
  edits don't silently degrade the global properties of the project
  (size, branching, nesting, coupling, duplication, test reference
  coverage).
- *Act*: meaning-preserving symbol renames and moves, so the agent can
  refactor across files without hand-tracking every reference.

Both modes are designed for an agent loop, not a TTY: stable prefixes,
predictable exit codes, and batch-friendly invocations.

---

## Problem statement

Editors that touch code one file at a time — humans, but especially
LLMs — tend to optimize for the patch in front of them. Over many such
patches, units grow, dependency graphs thicken, copy/paste accumulates,
tests fall behind, and renames either get skipped or get done by
text-replace with predictable wreckage.

Existing linters mostly police *style* and *language correctness*; they
do not police *structural drift* and they do not perform semantic
edits. `kiss` fills both gaps:

1. Given a tree of source files in a supported language, measure
   structural properties per code unit and across the project, compare
   them to thresholds (defaulted, mimicked, or clamped from the repo),
   and emit a stable line-oriented report.
2. Given a fully-qualified source symbol and a target name (and
   optionally a destination file), produce a precise, deterministic
   plan of edits that renames or moves that symbol everywhere it is
   referenced — without false positives on shadowed or unrelated
   same-named identifiers — and apply that plan transactionally.

---

## Stable vocabulary

Measurement vocabulary

- **Code unit**: smallest grain at which per-unit metrics are
  reported (function, method, or type, depending on metric).
- **Metric**: a named numeric property of a code unit or of the
  project (e.g. `statements`, `branches`, `graph_edges`).
- **Threshold**: configured upper (or lower) bound on a metric.
- **Violation**: emitted line indicating a metric crossed its
  threshold for a specific code unit.
- **Gate**: a violation class that, by default, halts further
  reporting until satisfied. Currently only the test-coverage gate
  exists.
- **Universe**: set of source files used to compute graph-level and
  coverage-level facts.
- **Focus**: subset of the universe whose violations are reported.
  Unspecified focus = universe.
- **Module**: file-level node in the dependency graph. One source
  file is one module.
- **Shared metric**: metric required to be emitted by both `check`
  (per-unit enforcement) and `stats` (distribution reporting).
- **Snapshot baseline**: recorded global metric values used by
  `shrink` to detect regression of non-targeted globals.
- **Strict-by-default**: built-in defaults reject typical "messy"
  code; adoption on existing repos is via clamp/mimic, not via loose
  defaults.

Refactoring vocabulary

- **Query**: the source-side identifier of the symbol to rename or
  move, given as `path::name`, `path::Type.method`, etc.
- **Target name**: the bare identifier the symbol should bear after
  the operation.
- **Definition span**: the byte range in the source file that bounds
  the symbol's defining construct (function, class/impl item, type).
- **Reference**: a use site of the symbol that must be rewritten in
  lockstep with the definition rename.
- **Plan**: ordered set of byte-range edits across one or more files
  that, applied together, perform the rename (and optionally move).
- **Transactional apply**: an all-or-nothing application of a plan;
  partial failure leaves the working tree unchanged.
- **Dry-run mode**: produce and report the plan without writing.
- **JSON mode**: emit the plan in a machine-stable structured form
  rather than human-oriented text.
- **AST analysis path**: the parser-backed resolution of definition
  and references using a real syntactic model of the file.
- **Lexical fallback**: a coarser identifier-scan path used only when
  the AST path cannot be constructed (parse failure), and only after
  an explicit warning.

---

## Objectives

### Primary objective

- Produce a **machine-parseable, deterministic** report of structural
  violations and, separately, a **machine-parseable, deterministic**
  plan of correctness-preserving symbol edits, both fast enough to
  run in an agent's inner edit/test loop.

### Secondary objectives (priority order)

1. **Cross-language consistency.** A metric name carries the same
   meaning in every supported language. Symbol resolution semantics
   (definition, scope, shadowing) are described in language-agnostic
   terms wherever possible; intentional asymmetries are declared.
2. **Synchronization between `check` and `stats`.** Any value `stats`
   can report for a shared metric must be reachable as a `check`
   violation given a low enough threshold, and vice versa.
3. **Strict-by-default, gradually adoptable.** Defaults are tight;
   `clamp` and `mimic` exist so existing codebases can opt in
   without first rewriting everything.
4. **Global analysis, local reporting.** Graph and coverage facts
   are computed over the universe but reports are restricted to the
   focus, so an agent only sees what's actionable for its current
   edit.
5. **Constrained minimization.** `shrink` provides a way to drive
   one global metric down without letting any other global metric
   grow.
6. **Precision before reach.** `mv` prefers refusing to rename (or
   loudly degrading) over silently producing wrong edits. False
   positives on shadowed or unrelated same-named identifiers are
   contract violations.

---

## Constraints (must-haves)

Each constraint is stated as goal / measurement / pass condition so a
reviewer can mechanically check it.

### Measurement-surface constraints

#### M1) Output is line-oriented and stably prefixed

- **Goal**: every report line a downstream parser must understand
  starts with a fixed prefix from a small, closed set.
- **Measurement**: run `kiss check` and `kiss show-tests` on a
  corpus and partition stdout lines by leading token before the
  first `:` or whitespace.
- **Pass condition**: every non-blank, non-indented stdout line
  begins with one of the documented prefixes (currently
  `Analyzed:`, `VIOLATION:<metric>:`, `GATE_FAILED:<gate>:`,
  `NO VIOLATIONS`, `TEST:`, `UNTESTED:`). Unknown prefixes count as
  a contract break.

#### M2) Exit code contract

- **Goal**: exit code is sufficient to drive an agent loop without
  parsing stdout for success/failure.
- **Measurement**: invoke each command with both clean and dirty
  inputs and observe `$?`.
- **Pass condition**:
  - `kiss check`: 0 iff no violations and no gate failure; 1
    otherwise.
  - `kiss shrink`: 0 iff target met and no other global regressed
    and no `check` violation; 1 otherwise.
  - All other measurement subcommands: 0 on successful execution
    regardless of measured values; 1 on operational error (invalid
    paths, no source files, bad config, write failure).

#### M3) Determinism

- **Goal**: same inputs ⇒ byte-identical stdout (modulo line
  ordering rules the command itself defines).
- **Measurement**: run a measurement subcommand twice on the same
  immutable tree with the same config; diff stdout.
- **Pass condition**: empty diff.

#### M4) Inner-loop performance

- **Goal**: `kiss check` is cheap enough to run alongside tests and
  formatters on every iteration.
- **Measurement**: wall-clock time on a corpus of representative
  repositories of known size.
- **Pass condition**: scaling is at most linear in source size; on
  a small/medium repo (≲ 100k LOC) the command completes in a time
  comparable to a fast linter pass on the same tree. Concrete
  numeric thresholds live in performance tests, not here.

#### M5) Universe-vs-focus invariant

- **Goal**: report is restricted to focus paths, but facts (graph
  metrics, coverage) are computed from the universe.
- **Measurement**: run `kiss check U F` where `F ⊂ U`; collect the
  set of files appearing in violation lines.
- **Pass condition**: every reported file is in `F`, *and* graph-
  level metrics for files in `F` are identical to the values they
  would have if computed against `U` alone.

#### M6) Cross-command metric synchronization

- **Goal**: for every shared metric, a value reportable by `stats`
  is reachable as a `check` violation given a sufficiently low
  threshold.
- **Measurement**: run `stats --all` and `check` (with all shared
  thresholds set to 0) on the same synthetic corpus.
- **Pass condition**: every nonzero `STAT` line for a shared metric
  has a corresponding `VIOLATION` line for the same `(metric, code
  unit)`, and vice versa. Intentional asymmetries are enumerated in
  code with rationale and excluded from this comparison.

#### M7) Cross-language metric semantics

- **Goal**: a metric name has the same operational definition in
  every supported language.
- **Measurement**: per-metric definition tables in code/tests.
- **Pass condition**: for each shared metric, no language emits a
  value violating the documented definition. Language-specific
  metrics are explicitly named as such.

#### M8) Configuration precedence

- **Goal**: config resolution is layered, predictable, and
  overridable.
- **Measurement**: load config with various combinations of
  `~/.kissconfig`, `./.kissconfig`, and `--config FILE` present.
- **Pass condition**: layers are merged in the order
  built-in defaults → `~/.kissconfig` → `./.kissconfig` → `--config`,
  later layers overriding earlier ones key-by-key. A missing layer
  is a no-op, not an error.

#### M9) Strict-by-default

- **Goal**: built-in defaults catch typical structural problems
  without the user having to opt in.
- **Measurement**: run `kiss check` with no config on a synthetic
  "messy" corpus.
- **Pass condition**: the messy corpus produces violations under
  defaults.

#### M10) Shrink monotonicity

- **Goal**: `kiss shrink` succeeds only when the targeted global
  metric has actually moved toward the target *and* no non-targeted
  global has grown beyond its baseline.
- **Measurement**: set a target, modify the tree, run `kiss shrink`.
- **Pass condition**: success iff `target_metric ≤ target_value`
  and, for every non-targeted global, `current ≤ baseline`.

### Refactoring-surface constraints (`kiss mv`)

#### R1) AST-first analysis with declared fallback

- **Goal**: definition and reference resolution use a parser-backed
  syntactic model. The lexical (identifier-scan) path is a fallback
  used only when parsing fails, and never silently.
- **Measurement**: feed both a parseable file and a syntactically
  broken file; observe stderr and the resulting plan.
- **Pass condition**: on parseable input, no rename decision is
  made by an identifier scan that the AST could have answered. On
  unparseable input, an explicit warning naming the file is emitted
  to stderr before any lexical-only edit is reported, and the
  affected file is either skipped or processed under the
  lexical-fallback path with that warning attached.

#### R2) Scope-aware symbol resolution

- **Goal**: `mv` rewrites only the binding the user named; shadowed
  or unrelated same-named identifiers are left alone.
- **Measurement**: regression suite covering nested-scope shadowing
  in both Python (LEGB, comprehension scopes) and Rust (block
  scopes, `let`-shadowing).
- **Pass condition**: every shadowing regression test passes; in
  particular, an inner re-binding with the same name as the target
  is not renamed, and an outer/sibling binding with the same name
  but different scope is not renamed.

#### R3) Receiver / method disambiguation

- **Goal**: when renaming a method, only call sites whose receiver
  is of the owning type are rewritten; same-named free functions or
  methods on unrelated types are not.
- **Measurement**: regression tests pairing `Type.helper` with a
  free `helper` function and with a `helper` method on an unrelated
  type, and exercising `obj.helper()` call sites.
- **Pass condition**: only the targeted owner's references are
  rewritten. Trait-receiver ambiguity that the AST cannot resolve
  must surface as a non-zero exit, not as a silent rewrite.

#### R4) Graceful degradation on malformed input

- **Goal**: malformed inputs never panic and never silently emit
  wrong edits.
- **Measurement**: feed a corpus including syntactically broken
  files.
- **Pass condition**: process does not panic; either an explicit
  warning is emitted and the file is skipped (no edits to it), or
  the lexical-fallback path is taken under R1's signaling
  requirement.

#### R5) Deterministic plans and edits

- **Goal**: same inputs ⇒ same plan ⇒ same on-disk result.
- **Measurement**: invoke `kiss mv` twice on the same immutable
  tree (once with `--dry-run`, once for real, and once more to
  re-verify); compare both the plan and the resulting trees.
- **Pass condition**: planned edits are totally ordered (by file
  path, then by start byte) and the ordering is stable across
  invocations; repeated runs produce byte-identical results.

#### R6) Transactional apply

- **Goal**: a failed apply leaves the working tree unchanged.
- **Measurement**: induce a write failure (e.g. read-only file)
  partway through a multi-file plan.
- **Pass condition**: no file in the plan is left in a
  partially-rewritten state; exit is non-zero with a diagnostic.

#### R7) Dry-run / apply equivalence

- **Goal**: `--dry-run` reports exactly the edit set that an actual
  run would perform on the same inputs.
- **Measurement**: run `--dry-run`, capture edits; run for real on
  a fresh copy of the same tree; compare the actual diff to the
  reported edits.
- **Pass condition**: identical edit set, file by file, byte range
  by byte range.

#### R8) Machine-stable JSON output

- **Goal**: `--json` mode is the parser-friendly contract for the
  plan; its shape changes are breaking changes.
- **Measurement**: validate `--json` output against a fixed schema
  in tests.
- **Pass condition**: schema validates; no field is renamed,
  removed, or repurposed without a coordinated contract bump.

#### R9) Single parse per file per invocation

- **Goal**: planning never reparses the same file more than once
  per invocation; this is what keeps `mv` cheap enough for the
  inner loop.
- **Measurement**: instrument the parser with a counter per file
  path during a planning run.
- **Pass condition**: for every file involved in the plan, the
  parser-invocation count is ≤ 1 per language.

---

## Non-goals

- **Style/formatting.** Whitespace, naming conventions, import
  order, and language-idiom checks belong to formatters and
  language-specific linters.
- **Runtime test coverage.** Coverage in `kiss` is a static
  test-reference check. Instrumented runtime coverage is out of
  scope.
- **Composite "code health" scores.** No God-Class index, no LCOM,
  no rolled-up quality grade. Only component metrics.
- **Library API.** `kiss` is shipped as a CLI. The `[lib]` target
  exists for internal modularity; it is not a supported public API.
- **TTY-friendly pretty output.** The contract is for parsers;
  humans are second-class consumers of stdout.
- **Languages other than Python and Rust** for either surface.
  Adding a language is a contract-level change.
- **Symbol-level dependency graphs** for the measurement surface.
  The dependency graph is module-level by design (file = node);
  finer-grained graphs are excluded as too expensive and too noisy
  for the inner loop. (This is unrelated to `mv`'s symbol-level
  resolution, which is per-invocation, not a global graph.)
- **Cross-language refactoring.** `kiss mv` operates within one
  language at a time; it does not chase a Python symbol into Rust
  bindings or vice versa.
- **Whole-program type inference for `mv`.** Disambiguation that
  would require a full type system (e.g. resolving an
  arbitrarily-typed `obj.helper()` purely from inference) is out
  of scope; such cases must surface as failures, not guesses.
- **Parser/library choices, parallelism mechanism, on-disk
  formats.** These are implementation details and may change
  without changing the contract above.

---

## Design principles

- **Contract before mechanism.** A change to output prefixes, exit
  codes, shared-metric names, or `mv`'s `--json` shape is a
  breaking change. A change to how a value is computed or how a
  rename is resolved is not, as long as the result still satisfies
  the documented contract.
- **Single computation path per metric.** A metric is computed in
  one place and consumed by both `check` and `stats`; no parallel
  re-implementation is permitted.
- **Single resolution path per file in `mv`.** A file is parsed at
  most once per invocation per language, and the AST path is
  authoritative whenever it succeeds.
- **Asymmetries are declared, not implicit.** Any metric emitted by
  one of `check`/`stats` but not the other is enumerated in code
  with a one-line rationale. Any case where `mv` falls back from
  AST to lexical resolution is signaled to the user, not hidden.
- **Fail closed on operational errors.** Bad paths, missing files,
  malformed config, write failures: exit 1, write to stderr, do
  not pretend to succeed. For `mv`, this extends to "ambiguous
  reference the AST cannot resolve" — refuse, don't guess.
- **Bounded per-file work.** Parsing and metric computation for one
  file must not require global state beyond what the graph/coverage
  layers provide; this keeps cost roughly linear in source size and
  makes file-level parallelism safe.
- **Quiet success.** A clean measurement run prints exactly one
  terminal sentinel line on stdout (`NO VIOLATIONS`); a clean `mv`
  run with no edits to make exits 0 with an empty plan. Auxiliary
  diagnostics go to stderr.
- **Defaults encode taste, not minimums.** The default config is a
  direction, not the lowest passable bar.
- **Precision before reach.** It is better for `mv` to refuse to
  rename a binding than to rename it incorrectly.

---

## Acceptance checklist

A change to `kiss` is acceptable only if it preserves the following.
Reviewers should walk this list before approving.

- **Prefix set unchanged.** No new top-level stdout prefix on the
  measurement surface is introduced, removed, or renamed without a
  corresponding update to M1.
- **Exit codes unchanged** per M2 for every measurement subcommand
  the change touches; per R6 for `mv`.
- **Determinism preserved** (M3, R5): repeated runs on the same
  input produce byte-identical results.
- **Universe/focus invariant preserved** (M5): focus narrows
  reporting only, never analysis.
- **Sync invariant preserved** (M6): the cross-command sync test
  still passes; any newly added shared metric is wired into both
  `check` and `stats`, or explicitly declared asymmetric with
  rationale.
- **Cross-language semantics preserved** (M7): a shared metric
  still has one definition.
- **Config precedence preserved** (M8).
- **Shrink monotonicity preserved** (M10) for any change touching
  `shrink`.
- **`mv` precision preserved** (R1–R4): no regression in scope,
  receiver, or fallback behavior; ambiguity surfaces, not guesses.
- **`mv` transactional and dry-run guarantees preserved** (R6,
  R7): partial writes never persist; `--dry-run` matches the real
  run.
- **`mv --json` schema unchanged** (R8) without a coordinated
  contract bump.
- **Single-parse invariant preserved** (R9) for any change to the
  `mv` planning path.
- **No implementation detail leaks into this document.** Library
  names, parallelism mechanism, specific algorithms, file paths,
  function names, and struct names do not appear here. (If a future
  revision adds a Catalog of code-level entities, this checklist
  must also gain a literal **"Catalog parity"** clause and the
  catalog itself must move in lockstep with the code.)
- **Non-goals respected.** A change that adds behavior outside the
  Non-goals list above requires updating the Non-goals list first,
  with a one-line rationale for the scope expansion.
