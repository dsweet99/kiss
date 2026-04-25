## `kiss mv` fixture conventions

- Each fixture repo should be valid before mutation.
- Python fixtures should pass `python -m compileall -q .` and `python -m pytest -q`.
- Keep fixtures small and realistic enough to exercise rename and move scenarios.
- If a deterministic or ignored heavy-tier test fails, reduce the failing project to the fewest files and symbols that still reproduce the bug.
- Add the minimized test to `tests/cases/symbol_mv_regressions*.rs` before or alongside the fix.
- Prefer one bug per regression test name.
