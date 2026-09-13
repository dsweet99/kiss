"""Time kiss test on ruff with a code cache then again with a warm cache."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

RUFF_REPO = (ROOT.parent / "repos" / "ruff").resolve()
# VISION.md: each eval runs in under 60s.
EVAL_TIMEOUT_S = 55


def _clear_runtime_test_cache(repo: Path) -> None:
    """Clear Python/runtime caches only.

    Do not delete rust_llvm_cov_cache: rebuilding it on ruff takes many minutes
    and violates the eval time budget.
    """
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    (kiss_dir / "cov_records_cache.json").unlink(missing_ok=True)


def _ensure_code_cache(repo: Path, env: dict[str, str]) -> None:
    outcome = run(
        "kiss-check-ruff-code-cache",
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


def timing_kiss_test() -> None:
    """Cold then warm `kiss test --lang python` on ruff within the eval budget.

    Full Rust llvm-cov on ruff exceeds 60s when caches are cold, so this eval
    times the Python path against the large ruff tree (workspace planning +
    Python selectors) without forcing a multi-minute Rust rebuild.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert RUFF_REPO.is_dir(), f"ruff repo missing: {RUFF_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(RUFF_REPO)
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(RUFF_REPO, env)
    _clear_runtime_test_cache(RUFF_REPO)
    cold = run(
        "kiss-test-ruff-cold",
        [str(KISS), "test", "--lang", "python"],
        RUFF_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    warm = run(
        "kiss-test-ruff-warm",
        [str(KISS), "test", "--lang", "python"],
        RUFF_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test() -> None:
    report_eval(timing_kiss_test)
