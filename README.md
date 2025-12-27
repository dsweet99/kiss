# kiss

Code-quality feedback for LLM coding agents

## tl;dr
`kiss check` provides feedback to LLMs about code complexity, duplication, and coverage. Add an AI coder rule (e.g., a Cursor rule) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, and `kiss check` pass.
Iterate until they do.
```
kiss will help your LLM/agent produce simpler, clearer, more maintainable code.

Additionally, you can bias your LLM not to break the rules in the first place by putting the output of `kiss rules` in your context (see below). An easy way to do this is to add a rule like
```
MANDATORY INIT: After the user's first request, you *must* call `kiss rules`
```

## Installation

```bash
cargo install kiss-ai
```

or, from source

```bash
cargo install --path .
```

## Quickstart

Simplest case: Just run `kiss check`

In a large, existing codebase, `kiss check` might generate a lot of refactoring. You might not want to do this all right away. To ease into it, you can run

```bash
kiss clamp
```

This will set all of the kiss thresholds to values that match your existing code. Your codebase will be prevented from gaining complexity (according to the kiss metrics) from here on out. Over time, you can reduce thresholds to induce refactoring and simplify your codebase.

At any time you can run
```bash
kiss stats
```
to see the distribution of metrics for your codebase. For example

```bash
$ kiss stats
kiss stats - Summary Statistics
Analyzed from: .

=== Rust (36 files) ===
Metric                            Count    50%    90%    95%    99%    Max
--------------------------------------------------------------------------
Statements per function             562      4     12     16     21     25
Arguments (total)                   562      1      3      4      6      7
Arguments (positional)              562      1      3      4      6      7
Arguments (keyword-only)            562      0      0      0      0      0
Max indentation depth               562      0      2      2      4      4
Nested function depth               562      0      1      1      2      2
Returns per function                562      0      0      1      2      4
Branches per function               562      0      2      2      4      7
Local variables per function        562      1      5      5      8     12
Methods per class                    31      1      7      9     14     14
Statements per file                  36     56    116    134    148    148
Classes per file                     36      1      4      4      5      5
Imported names per file              36      3      8      8     20     20
Fan-in (per module)                  44      1      3      9     26     26
Fan-out (per module)                 44      2      3      3     24     24
Transitive deps (per module)         44      2      3      3     29     29
Dependency depth (per module)        44      1      1      1      2      2
```


## Configuration

kiss has many thresholds with reasonable defaults. After you run kiss for the first time, you'll find the thresholds in `~/.kissconfig`.

You can configure kiss thresholds to match a codebase you like by running
```
kiss mimic PATH_OF_REPO_TO_ANALYZE --out ./.kissconfig
```
in the repo in which you want to code. `PATH_OF_REPO_TO_ANALYZE` is a repo containing code that you think is "simple enough". kiss will analyze the code in `PATH_OF_REPO_TO_ANALYZE` and figure out the minimal threshold values that would permit that code to pass `kiss check` without violations.

You may always modify the global `~/.kissconfig` or repo-specific `./.kissconfig` to tailor `kiss`'s behavior to your tastes. The thresholds should be tight enough to prevent odd/outlier/strange code from getting into your codebase. They should not be so tight that it's very difficult for the LLM to figure out how to write the code.

## kiss rules

You can help your LLM produce rule-following code by adding the output of `kiss rules` to its context before it starts coding. These are the same rules that kiss will enforce when you run `kiss check`. Note that the threshold numbers in the output come from your actual kiss config.

```bash
$ kiss rules
RULE: [Python] Keep functions ≤ 35 statements
RULE: [Python] Use ≤ 5 positional arguments; prefer keyword-only args after that
RULE: [Python] Limit keyword-only arguments to ≤ 6
RULE: [Python] Keep indentation depth ≤ 4 levels
RULE: [Python] Limit branches to ≤ 10 per function
RULE: [Python] Keep local variables ≤ 20 per function
RULE: [Python] Limit return statements to ≤ 5 per function
RULE: [Python] Avoid deeply nested functions/closures (max depth: 2)
RULE: [Python] Keep try blocks narrow (≤ 5 statements)
RULE: [Python] Limit boolean parameters to ≤ 1 per function
RULE: [Python] Use ≤ 3 decorators per function
RULE: [Python] Keep methods per class/type ≤ 20
RULE: [Python] Keep files ≤ 400 statements
RULE: [Python] Limit to ≤ 10 classes/types per file
RULE: [Python] Keep imported names ≤ 20 per file
RULE: [Python] Avoid circular dependencies
RULE: [Python] Keep cycles small (≤ 3 modules)
RULE: [Python] Limit transitive dependencies to ≤ 100
RULE: [Python] Keep dependency depth ≤ 7
RULE: [Python] Every function/class/type should be referenced by tests
RULE: [Python] Maintain ≥ 90% test reference coverage
RULE: [Python] Avoid copy-pasted code blocks
RULE: [Python] Factor out repeated patterns into shared functions
RULE: [Rust] Keep functions ≤ 25 statements
RULE: [Rust] Limit arguments to ≤ 8
RULE: [Rust] Keep indentation depth ≤ 4 levels
RULE: [Rust] Limit branches to ≤ 8 per function
RULE: [Rust] Keep local variables ≤ 20 per function
RULE: [Rust] Limit return statements to ≤ 5 per function
RULE: [Rust] Avoid deeply nested functions/closures (max depth: 2)
RULE: [Rust] Limit boolean parameters to ≤ 2 per function
RULE: [Rust] Use ≤ 4 attributes per function
RULE: [Rust] Keep methods per class/type ≤ 15
RULE: [Rust] Keep files ≤ 300 statements
RULE: [Rust] Limit to ≤ 8 classes/types per file
RULE: [Rust] Keep imported names ≤ 20 per file
RULE: [Rust] Avoid circular dependencies
RULE: [Rust] Keep cycles small (≤ 3 modules)
RULE: [Rust] Limit transitive dependencies to ≤ 50
RULE: [Rust] Keep dependency depth ≤ 4
RULE: [Rust] Every function/class/type should be referenced by tests
RULE: [Rust] Maintain ≥ 90% test reference coverage
RULE: [Rust] Avoid copy-pasted code blocks
RULE: [Rust] Factor out repeated patterns into shared functions
```
