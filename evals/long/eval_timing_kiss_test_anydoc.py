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
# Mirrors src/rust_llvm_cov_runner/build_depot.rs::PRESERVED_CACHE_*.
_PRESERVED_LLVM_DIRS = frozenset({"build", "locks"})
_PRESERVED_LLVM_FILES = frozenset(
    {
        "binary_digest_memo.json",
        "runner_resolve_cache.json",
        "input_mtime_seal.json",
    }
)


def _clear_semantic_llvm_cache(cache_root: Path) -> None:
    """Drop coverage evidence but keep the instrumented cargo build depot.

    Wiping all of `rust_llvm_cov_cache` forces an instrumented cargo rebuild
    (~40s+) and sits on the 55s timeout cliff. Semantic-only clear still makes
    the next run a real nextest+llvm-cov miss while staying well under budget.
    """
    if not cache_root.is_dir():
        return
    for entry in list(cache_root.iterdir()):
        if entry.name in _PRESERVED_LLVM_DIRS or entry.name in _PRESERVED_LLVM_FILES:
            continue
        if entry.is_dir():
            shutil.rmtree(entry, ignore_errors=True)
        else:
            entry.unlink(missing_ok=True)


def _clear_runtime_test_cache(repo: Path) -> None:
    """Clear kiss test caches so the next run is a cold coverage miss.

    Do not delete cargo `target/` or the llvm `build/` depot: rebuilding the
    instrumented binary pushes the cold run over the eval budget.
    """
    kiss_dir = repo / ".kiss"
    shutil.rmtree(kiss_dir / "rslip_cache", ignore_errors=True)
    _clear_semantic_llvm_cache(kiss_dir / "rust_llvm_cov_cache")
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


def _ensure_instrumented_build(
    repo: Path, env: dict[str, str], argv: list[str]
) -> None:
    """Populate the llvm build depot when missing so timed cold stays under budget."""
    build_target = repo / ".kiss" / "rust_llvm_cov_cache" / "build" / "target"
    if build_target.is_dir():
        return
    outcome = run(
        "kiss-test-anydoc-ensure-build",
        argv,
        repo,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    assert build_target.is_dir(), (
        "instrumented llvm build depot missing after ensure-build\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )


def _cargo_nextest_argv() -> list[str]:
    # ANYDOC_RUST_TARGET is a file path like tests/robustness.rs → nextest --test robustness
    stem = Path(ANYDOC_RUST_TARGET).stem
    return ["cargo", "nextest", "run", "--test", stem]


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
    argv = [str(KISS), "test", "--lang", "rust", ANYDOC_RUST_TARGET]
    _ensure_code_cache(ANYDOC_REPO, env)
    _ensure_instrumented_build(ANYDOC_REPO, env, argv)
    _clear_runtime_test_cache(ANYDOC_REPO)
    cold = run(
        "kiss-test-anydoc-cold",
        argv,
        ANYDOC_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    assert "PASS (cached):" not in cold.stdout, (
        "cold run unexpectedly used a full cache hit\n"
        f"stdout:\n{cold.stdout}\nstderr:\n{cold.stderr}"
    )
    warm = run(
        "kiss-test-anydoc-warm",
        argv,
        ANYDOC_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    nextest = run(
        "cargo-nextest-anydoc-baseline",
        _cargo_nextest_argv(),
        ANYDOC_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    if nextest.elapsed <= 0:
        raise RuntimeError("cargo nextest baseline elapsed must be positive")
    ratio = cold.elapsed / nextest.elapsed
    emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
    emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")
    emit_eval("cargo_nextest_elapsed_s", "SMALLER", f"{nextest.elapsed:.4f}")
    emit_eval("kiss_test_to_nextest_ratio", "SMALLER", f"{ratio:.4f}")
    # RuntimeError (not AssertionError): report_eval swallows assert failures.
    if ratio >= 2.0:
        raise RuntimeError(
            f"kiss test / cargo nextest ratio {ratio:.4f} is not under 2 "
            f"(kiss_cold={cold.elapsed:.4f}s nextest={nextest.elapsed:.4f}s)"
        )


def eval_timing_kiss_test_anydoc() -> None:
    report_eval(timing_kiss_test_anydoc)
