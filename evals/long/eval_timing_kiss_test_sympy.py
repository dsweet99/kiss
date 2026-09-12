"""Time kiss test on sympy with a code cache then again with a warm cache."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

SYMPY_REPO = (ROOT.parent / "repos" / "sympy").resolve()


def _clear_test_cache(repo: Path) -> None:
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rust_llvm_cov_cache", ignore_errors=True)
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    (kiss_dir / "cov_records_cache.json").unlink(missing_ok=True)


def _ensure_code_cache(repo: Path, env: dict[str, str]) -> None:
    outcome = run(
        "kiss-check-sympy-code-cache",
        [str(KISS), "check", "."],
        repo,
        env,
        expected=0,
        timeout=1_200,
    )
    assert "Analyzed:" in outcome.stdout, (
        f"kiss check did not finish analysis (rc={outcome.returncode})\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )


def timing_kiss_test_sympy() -> None:
    """Run `kiss test` with a code cache then run it again with warm cache in sympy."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert SYMPY_REPO.is_dir(), f"sympy repo missing: {SYMPY_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(SYMPY_REPO)
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(SYMPY_REPO, env)
    _clear_test_cache(SYMPY_REPO)
    cold = run(
        "kiss-test-sympy-cold",
        [str(KISS), "test"],
        SYMPY_REPO,
        env,
        expected=0,
        timeout=1_200,
    )
    warm = run(
        "kiss-test-sympy-warm",
        [str(KISS), "test"],
        SYMPY_REPO,
        env,
        expected=0,
        timeout=1_200,
    )
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test_sympy() -> None:
    report_eval(timing_kiss_test_sympy)
