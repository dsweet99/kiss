# kiss

Code-quality feedback for LLM coding agents.

## tl;dr
`kiss` provides feedback to LLMs about code complexity, duplication, and coverage. Add an AI coder rule (e.g., a Cursor rule) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, and `kiss` pass.
Iterate until they do.
```
`kiss` will help your LLM/agent produce simpler, clearer, more maintainable code.  

Additionally, you can bias your LLM to not break the rules in the first place by putting the output of the command `kiss rules` in your context (see below). An easy way to do this is to add a rule like
```
MANDATORY INIT: After the user's first request, you *must* call `kiss rules`
```



## Installation

```bash
cargo install --path .
```


## Configuration
`kiss` has many thresholds with reasonable defaults. After you run kiss for the first time, you'll find the thresholds in `~/.kissconfig`.

You can configure `kiss` thresholds to match a codebase you like by running
```
kiss mimic PATH_OF_REPO_TO_ANALYZE --out ./.kissconfig
```
in the repo in which you want to code. `PATH_OF_REPO_TO_ANALYZE` is a repo containing code that you think is "simple enough". `kiss` will analyze the code in `PATH_OF_REPO_TO_ANALYZE` and figure out the minimal threshold values that would permit that code to pass `kiss` without violations.

You may always modify the global `~/.kissconfig` or repo-specific `./.kissconfig` to tailor `kiss`'s behavior to your tastes. The thresholds should be tight enough to prevent odd/outlier/strange code from getting into your code base. They should be so tight that it's very difficult for the LLM to figure out how to write the code.



## `kiss rules`

You can help your LLM produce rule-following code by adding the output of `kiss rules` to its context before it starts coding. Note that the threshold numbers in the output come from your actual kiss config.

```
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
RULE: [Python] Keep files ≤ 300 lines
RULE: [Python] Limit to ≤ 10 classes/types per file
RULE: [Python] Keep imports ≤ 20 per file
RULE: [Python] Avoid circular dependencies
RULE: [Python] Keep cycles small (≤ 3 modules)
RULE: [Python] Limit transitive dependencies to ≤ 30
RULE: [Python] Keep dependency depth ≤ 6
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
RULE: [Rust] Keep files ≤ 300 lines
RULE: [Rust] Limit to ≤ 8 classes/types per file
RULE: [Rust] Keep imports ≤ 20 per file
RULE: [Rust] Avoid circular dependencies
RULE: [Rust] Keep cycles small (≤ 3 modules)
RULE: [Rust] Limit transitive dependencies to ≤ 30
RULE: [Rust] Keep dependency depth ≤ 6
RULE: [Rust] Every function/class/type should be referenced by tests
RULE: [Rust] Maintain ≥ 90% test reference coverage
RULE: [Rust] Avoid copy-pasted code blocks
RULE: [Rust] Factor out repeated patterns into shared functions
```
