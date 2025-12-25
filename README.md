# kiss

Code-quality metrics for LLM coding agents.

## tl;dr
`kiss` provides feedback to LLMs about code complexity, duplication, and coverage. Add an AI coder rule (e.g., a Cursor rule) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, and `kiss` pass.
Iterate until they do.
```
`kiss` will help your LLM/agent produce simpler, clearer, more maintainable code.  

Additionally, you can bias your LLM to not break the rules in the first place by putting the output of `kiss rules` in your context. An easy way to do this is to add a rule like
```
MANDATORY INIT: After the user's first request, you *must* call `kiss rules`
```



## Installation

```bash
cargo install --path .
```


## Configuration
`kiss` has many thresholds with reasonable defaults. After you run kiss for the first time, you'll find the thresholds in `~/.kissconfig`.

You can configure `kiss` to match a codebase you like by running
```
kiss mimic PATH_OF_REPO_TO_ANALYZE --out ./.kissconfig
```
in the repo you want in which you want to code. `PATH_OF_REPO_TO_ANALYZE` is a repo containing code that you think is "simple enough". `kiss` will analyze the code in `PATH_OF_REPO_TO_ANALYZE` and figure out the minimal threshold values that would permit that code to pass `kiss` without violations.

You may always modify `./.kissconfig` to tailor it to your tastes.

