"""Time kiss check/test on anydoc (mixed Rust + Python) cold then warm."""

from __future__ import annotations

import os
import shutil
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

ANYDOC_REPO = (ROOT.parent / "repos" / "anydoc").resolve()
# VISION.md: each eval runs in under 60s.
EVAL_TIMEOUT_S = 55
# Full anydoc Rust llvm-cov from a cold cargo target exceeds the budget; a single
# robustness test still exercises the mixed tree after kiss check analyzes both
# languages. Nested Python bindings need a built extension and path layout that
# breaks kiss pytest nodeids, so this eval times the Rust path.
ANYDOC_RUST_TARGET = "tests/robustness.rs"


def _clear_runtime_test_cache(repo: Path) -> None:
    """Clear kiss test caches so the next run is a cold coverage miss.

    Do not delete cargo `target/`: rebuilding dependencies pushes the cold run
    over the eval budget. Clearing `rust_llvm_cov_cache` is safe on anydoc
    because the tree is small enough that a warm cargo target still finishes
    llvm-cov well under 55s (unlike ruff).
    """
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    shutil.rmtree(kiss_dir / "rust_llvm_cov_cache", ignore_errors=True)
    (kiss_dir / "cov_records_cache.json").unlink(missing_ok=True)


def _ensure_code_cache(repo: Path, env: dict[str, str]) -> None:
    outcome = run(
        "kiss-check-anydoc-code-cache",
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


def timing_kiss_test_anydoc() -> None:
    """Cold then warm scoped `kiss test` on mixed anydoc within the eval budget.

    `kiss check` analyzes the Rust sources and nested Python bindings. The timed
    test path is Rust-only: Python bindings require a maturin-built extension and
    currently fail kiss collection path rewriting under `python/tests/`.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert ANYDOC_REPO.is_dir(), f"anydoc repo missing: {ANYDOC_REPO}"
    env = os.environ.copy()
    env["PYTHONPATH"] = str(ANYDOC_REPO / "python")
    env.pop("RUSTFLAGS", None)
    _ensure_code_cache(ANYDOC_REPO, env)
    _clear_runtime_test_cache(ANYDOC_REPO)
    argv = [str(KISS), "test", "--lang", "rust", ANYDOC_RUST_TARGET]
    cold = run(
        "kiss-test-anydoc-cold",
        argv,
        ANYDOC_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    warm = run(
        "kiss-test-anydoc-warm",
        argv,
        ANYDOC_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test_anydoc() -> None:
    report_eval(timing_kiss_test_anydoc)
