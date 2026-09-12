"""Time kiss test on ruff with a code cache then again with a warm cache."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

RUFF_REPO = (ROOT.parent / "repos" / "ruff").resolve()


def _clear_test_cache(repo: Path) -> None:
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rust_llvm_cov_cache", ignore_errors=True)
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    (kiss_dir / "cov_records_cache.json").unlink(missing_ok=True)


def _ensure_code_cache(repo: Path, env: dict[str, str]) -> None:
    outcome = run(
        "kiss-check-ruff-code-cache",
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


def timing_kiss_test() -> None:
    """Run `kiss test` with a code cache then run it again with warm cache in ruff."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert RUFF_REPO.is_dir(), f"ruff repo missing: {RUFF_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(RUFF_REPO)
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(RUFF_REPO, env)
    _clear_test_cache(RUFF_REPO)
    cold = run(
        "kiss-test-ruff-cold",
        [str(KISS), "test"],
        RUFF_REPO,
        env,
        expected=0,
        timeout=1_200,
    )
    warm = run(
        "kiss-test-ruff-warm",
        [str(KISS), "test"],
        RUFF_REPO,
        env,
        expected=0,
        timeout=1_200,
    )
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test() -> None:
    report_eval(timing_kiss_test)
