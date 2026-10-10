"""Time kiss test on a sympy core subset."""

from __future__ import annotations

import os

from evals._harness import KISS, ROOT, Outcome, emit_eval, report_eval, run

SYMPY_REPO = (ROOT.parent / "repos" / "sympy").resolve()
# Each eval is budgeted under 60s.
EVAL_TIMEOUT_S = 55
# Full sympy execution exceeds the eval budget; use a substantial core subset.
SYMPY_TARGETS = (
    "sympy/core/tests/test_basic.py",
    "sympy/core/tests/test_expr.py",
    "sympy/core/tests/test_symbol.py",
    "sympy/core/tests/test_numbers.py",
)


def _assert_subset_run_ok(outcome: Outcome) -> None:
    assert outcome.returncode == 0, (
        f"{outcome.name}: expected rc=0, got {outcome.returncode}\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )
    assert "passed" in outcome.stdout.lower() or "PASS" in outcome.stdout, (
        f"{outcome.name}: no pass summary\nstdout:\n{outcome.stdout}\n"
        f"stderr:\n{outcome.stderr}"
    )


def timing_kiss_test_sympy() -> None:
    """Time a scoped `kiss test` on sympy within the eval budget.

    The full sympy suite exceeds 60s, so this eval times a large core-test subset.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert SYMPY_REPO.is_dir(), f"sympy repo missing: {SYMPY_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(SYMPY_REPO)
    env.pop("RUSTFLAGS", None)
    argv = [str(KISS), "test", "--lang", "python", *SYMPY_TARGETS]
    outcome = run(
        "kiss-test-sympy",
        argv,
        SYMPY_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    _assert_subset_run_ok(outcome)
    emit_eval("kiss_test_elapsed_s", "SMALLER", f"{outcome.elapsed:.4f}")


def eval_timing_kiss_test_sympy() -> None:
    report_eval(timing_kiss_test_sympy)
