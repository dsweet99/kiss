"""Time kiss test on sympy with a code cache then again with a warm cache."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, Outcome, emit_eval, report_eval, run

SYMPY_REPO = (ROOT.parent / "repos" / "sympy").resolve()
# VISION.md: each eval runs in under 60s.
EVAL_TIMEOUT_S = 55
# Full sympy execution exceeds the eval budget; use a substantial core subset.
SYMPY_TARGETS = (
    "sympy/core/tests/test_basic.py",
    "sympy/core/tests/test_expr.py",
    "sympy/core/tests/test_symbol.py",
    "sympy/core/tests/test_numbers.py",
)


def _clear_runtime_test_cache(repo: Path) -> None:
    """Clear Python/runtime caches only (sympy has no Rust llvm-cov tree)."""
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    (kiss_dir / "cov_records_cache.json").unlink(missing_ok=True)


def _ensure_code_cache(repo: Path, env: dict[str, str]) -> None:
    outcome = run(
        "kiss-check-sympy-code-cache",
        [str(KISS), "check", "."],
        repo,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    assert "Analyzed:" in outcome.stdout, (
        f"kiss check did not finish analysis (rc={outcome.returncode})\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )


def _assert_subset_run_ok(outcome: Outcome) -> None:
    """Subset runs may exit 1 when the unit-test timing cache is incomplete."""
    assert "passed" in outcome.stdout.lower() or "PASS" in outcome.stdout, (
        f"{outcome.name}: no pass summary\nstdout:\n{outcome.stdout}\n"
        f"stderr:\n{outcome.stderr}"
    )
    if outcome.returncode == 0:
        return
    assert outcome.returncode == 1, (
        f"{outcome.name}: expected rc=0 or timing-incomplete rc=1, "
        f"got {outcome.returncode}\nstdout:\n{outcome.stdout}\n"
        f"stderr:\n{outcome.stderr}"
    )
    assert "unit-test timing cache is incomplete" in outcome.stderr, (
        f"{outcome.name}: unexpected rc=1\nstdout:\n{outcome.stdout}\n"
        f"stderr:\n{outcome.stderr}"
    )


def timing_kiss_test_sympy() -> None:
    """Cold then warm scoped `kiss test` on sympy within the eval budget.

    The full sympy suite exceeds 60s, so this eval times a large core-test
    subset that still exercises caching on the real sympy tree.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert SYMPY_REPO.is_dir(), f"sympy repo missing: {SYMPY_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(SYMPY_REPO)
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(SYMPY_REPO, env)
    _clear_runtime_test_cache(SYMPY_REPO)
    argv = [str(KISS), "test", "--lang", "python", *SYMPY_TARGETS]
    cold = run(
        "kiss-test-sympy-cold",
        argv,
        SYMPY_REPO,
        env,
        expected=None,
        timeout=EVAL_TIMEOUT_S,
    )
    _assert_subset_run_ok(cold)
    warm = run(
        "kiss-test-sympy-warm",
        argv,
        SYMPY_REPO,
        env,
        expected=None,
        timeout=EVAL_TIMEOUT_S,
    )
    _assert_subset_run_ok(warm)
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test_sympy() -> None:
    report_eval(timing_kiss_test_sympy)
