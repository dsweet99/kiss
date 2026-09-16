"""Time kiss test on ruff with a code cache then again with a warm cache."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

RUFF_REPO = (ROOT.parent / "repos" / "ruff").resolve()
EVAL_CONFIG = Path(__file__).with_name("ruff_timing.kissconfig")
# Intentional syntax-error fixtures (ruff_benchmark/resources, ty completion truth).
IGNORE_PREFIXES = ("resources", "ty_completion_eval", "ty_benchmark")
# VISION.md: each eval runs in under 60s.
EVAL_TIMEOUT_S = 55


def _ruff_kiss_cmd(*args: str) -> list[str]:
    """Python-only ruff commands that skip fixture trees and ruff's .kissconfig."""
    cmd = [
        str(KISS),
        *args,
        "--config",
        str(EVAL_CONFIG),
        "--lang",
        "python",
    ]
    for prefix in IGNORE_PREFIXES:
        cmd.extend(["--ignore", prefix])
    return cmd


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
        [*_ruff_kiss_cmd("check"), "."],
        repo,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    assert "Analyzed:" in outcome.stdout, (
        f"kiss check did not finish analysis (rc={outcome.returncode})\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )


def _seed_python_selector_cache(repo: Path, env: dict[str, str]) -> None:
    """Write the python-only workspace selector cache before timed runs.

    The first `kiss test` on this ignore set may still fail sibling gates; the
    selector file is what later timed runs use for a known-empty population.
    """
    run(
        "kiss-test-ruff-seed-selectors",
        _ruff_kiss_cmd("test"),
        repo,
        env,
        expected=None,
        timeout=EVAL_TIMEOUT_S,
    )


def timing_kiss_test() -> None:
    """Cold then warm `kiss test --lang python` on ruff within the eval budget.

    Full Rust llvm-cov on ruff exceeds 60s when caches are cold, so this eval
    times the Python path against the large ruff tree (workspace planning +
    Python selectors) without forcing a multi-minute Rust rebuild.

    `--ignore` skips intentional syntax-error fixtures. `--config` keeps the
    eval from writing language tables into ruff's .kissconfig.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert RUFF_REPO.is_dir(), f"ruff repo missing: {RUFF_REPO}"
    assert EVAL_CONFIG.is_file(), f"eval config missing: {EVAL_CONFIG}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(RUFF_REPO)
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(RUFF_REPO, env)
    _seed_python_selector_cache(RUFF_REPO, env)
    _clear_runtime_test_cache(RUFF_REPO)
    cold = run(
        "kiss-test-ruff-cold",
        _ruff_kiss_cmd("test"),
        RUFF_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    warm = run(
        "kiss-test-ruff-warm",
        _ruff_kiss_cmd("test"),
        RUFF_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test() -> None:
    report_eval(timing_kiss_test)
