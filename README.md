# kiss

Global code feedback for LLM coding agents

## tl;dr
`kiss check` provides feedback to LLMs about code complexity and duplication; `kiss test` refreshes and enforces cached runtime line coverage. Add an AI coder rule (e.g., in `AGENTS.md`) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, `kiss check`, and `kiss test` pass.
Iterate until they do.
```
kiss will help your agent produce simpler, clearer, more maintainable code. kiss works on Python and Rust.


## The Problem: Missing Global Context
LLMs operate locally, focusing on whatever code they are editing plus bits and pieces of other, relevant code. They ignore the overall structure of the codebase because they don't see it. Over time, code tends to be a little more tangled, a little less DRY, harder to read and harder to update. To counteract this, LLMs need global information about the codebase.

kiss attempts to provide that in the form of stats about files, functions, etc., code-graph metrics, detected duplication, and low runtime line coverage. `kiss check` stays fast and static; run `kiss test` when you need coverage enforcement. kiss's output is compact, so it won't bloat context. `orphan` (under `[test] orphan_detection`) is a flood-fill reachability check run by `kiss test` after coverage: a production unit that no test, main, or coverage-reached unit names. `kiss check` does not report orphans.

## Installation

```bash
cargo install kiss-ai
```

## Quickstart

In your repo root, run:
```bash
kiss check
```
If `.kissconfig` is missing, `kiss check` will write one with threshold set so that your repo *just* passes `kiss check`. Any future code complexity increases will be prevented.

When your LLM runs `kiss check` it will see whether any of the code it has written has violated a constraint. For example:
```
VIOLATION:positional_args:src/shipping.py:12:calculate_shipping: Function 'calculate_shipping' has 6 positional arguments (threshold: 3) Consider using keyword-only arguments, a config object, or the builder pattern.
```
Too many arguments is a [code smell](https://stackoverflow.com/questions/68069305/how-to-avoid-code-smell-too-many-parameters). kiss tells exactly where to find the problem and suggests solutions.

LLMs like to write long try/except blocks, which is terrible practice as it can hide errors and frustrate debugging.
```
VIOLATION:statements_per_try_block:src/api.py:40:process_batch: Function 'process_batch' has 12 statements in try block (threshold: 3) Keep try blocks narrow: wrap only the code that can raise the specific exception.
```

Finally, LLMs have a tendency to rewrite small functions rather than finding and reusing them in the codebase, so kiss has a built-in (very fast!) duplicate-code detector:
```
VIOLATION:duplication:src/users.py:10:create_user: 80% similar, 2 copies: [src/users.py:10-40, src/accounts.py:8-38]. Extract common code into a shared function.
```

## `kiss test`

`kiss test` runs your unit tests, then enforces line-level code coverage, and limites running time of unit tests. `kiss test` is designed to be an efficient and robust unit test runner for both Python and Rust. It supports
- Caching, to avoid reruns of working tests
- Parallelization, to speed up test running
- Separate interpreters for each Python test, to reduce test flakiness and failures of the test runner
- Timeouts with feedback for your agent so that it will write faster tests


---


## Exploring your code: `kiss stats` and `kiss viz`

At any time you can run
```bash
kiss stats
```
to see the distribution of metrics for your codebase. For example

```
$ kiss stats
kiss stats - Summary Statistics
Analyzed from: .
Config: defaults + ./.kissconfig (found)

Analyzed: N files, N code_units, N statements, N graph_nodes, N graph_edges
Violations: 0 duplicate, 0 orphan, 0 comment, 0 doc

=== Rust (N files) ===
metric_id                        N   p50   p90   p95   p99   max
----------------------------------------------------------------
statements_per_function        ...   ...   ...   ...   ...   ...
positional_args                ...   ...   ...   ...   ...   ...
```

The header columns are `metric_id`, `N`, `p50`…`max`. File counts and percentiles come from your tree.

If you notice some outliers, try editing `.kissconfig` then asking your LLM to make `kiss check` pass. Watch it refactor and simplify your codebase.

To visualize your code graph, try
```bash
kiss viz graph.md --zoom=0.25
```
This will create a Mermaid plot inside the markdown file graph.md (viewable in VSCode/Cursor). Graph nodes are modules (files). `--zoom=0.25` coarsens the graph by merging nodes. `--zoom=1.0` includes every module. `--zoom=0.0` produces the trivial graph with one node representing your entire codebase.

![Dependency graph example](images/graph.png)


## kiss rules

You can help your LLM produce rule-following code by adding the output of `kiss rules` (see below) to its context before it starts coding (e.g., in AGENTS.md):
```
FIRST STEP: After the user's first request, before doing anything else, call `kiss rules`
```


The rules that `kiss rules` dumps to stdout are enforced by `kiss check` for static rules and by `kiss test` for coverage, time, and test-count gates. Threshold numbers come from your `.kissconfig`, or from `--config FILE` when you pass that flag. Run `kiss rules` for the live catalog. Example line:

```
RULE: [Python] [positional_args <= 3] positional_args is the maximum number of positional parameters in a Python function definition.
```

Complexity and size maxima print `<= N` because a value equal to the configured maximum is legal.

