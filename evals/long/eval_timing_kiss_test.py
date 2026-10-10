"""Time kiss test on ruff."""

from __future__ import annotations

import os
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run

RUFF_REPO = (ROOT.parent / "repos" / "ruff").resolve()
EVAL_CONFIG = Path(__file__).with_name("ruff_timing.kissconfig")
# Intentional syntax-error fixtures (ruff_benchmark/resources, ty completion truth).
IGNORE_PREFIXES = ("resources", "ty_completion_eval", "ty_benchmark")
# Each eval is budgeted under 60s.
EVAL_TIMEOUT_S = 55


def _ruff_kiss_cmd(*args: str) -> list[str]:
    """Python-only ruff commands that skip fixture trees and ruff's .kissconfig.

    `kiss test` has no `--ignore` flag; it reads `[test] ignore` from EVAL_CONFIG.
    """
    cmd = [
        str(KISS),
        *args,
        "--config",
        str(EVAL_CONFIG),
        "--lang",
        "python",
    ]
    if args[0] == "check":
        for prefix in IGNORE_PREFIXES:
            cmd.extend(["--ignore", prefix])
    return cmd


def timing_kiss_test() -> None:
    """Time `kiss test --lang python` on ruff within the eval budget.

    A full Rust run on ruff exceeds 60s, so this eval times the Python path
    against the large ruff tree without forcing a multi-minute Rust rebuild.

    Ignore prefixes skip intentional syntax-error fixtures. `--config` keeps the
    eval from writing language tables into ruff's .kissconfig.
    """
    assert KISS.is_file(), f"local binary missing: {KISS}"
    assert RUFF_REPO.is_dir(), f"ruff repo missing: {RUFF_REPO}"
    # RuntimeError (not AssertionError): report_eval swallows asserts and would
    # exit 0 with empty timing metrics if this fixture were missing.
    if not EVAL_CONFIG.is_file():
        raise RuntimeError(f"eval config missing: {EVAL_CONFIG}")
    env = os.environ.copy()
    env["PYTHONPATH"] = str(RUFF_REPO)
    env.pop("RUSTFLAGS", None)
    outcome = run(
        "kiss-test-ruff",
        _ruff_kiss_cmd("test"),
        RUFF_REPO,
        env,
        expected=0,
        timeout=EVAL_TIMEOUT_S,
    )
    emit_eval("kiss_test_elapsed_s", "SMALLER", f"{outcome.elapsed:.4f}")


def eval_timing_kiss_test() -> None:
    report_eval(timing_kiss_test)
