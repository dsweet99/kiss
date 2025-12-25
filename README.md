# kiss

**Code-quality metrics for LLM coding agents.**

## tj;dr
`kiss` provides feedback to LLMs about code complexity, duplication, and coverage. Add an AI coder rule (e.g., a Cursor rule) like
```
When you write code, always make sure `pytest -sv tests`, `ruff check`, and `kiss` pass.
Iterate until they do.
```
`kiss` will help you LLM produce simpler, clearer, more maintainable code. Maybe you'll even see fewer bugs.
Additionally, you can bias your LLM to not break the rules in the first place by putting the output of `kiss rules` in your context. An easy way to do this is to add a rule like
```
MANDATORY INIT: After the user's first request, you *must* call `kiss rules`
```



## Installation

```bash
cargo install --path .
```
