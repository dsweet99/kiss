"""Time kiss check on a large-ish, complex tmp repo."""

from __future__ import annotations

import os
import shutil
import tempfile
from pathlib import Path

from evals._harness import KISS, ROOT, emit_eval, report_eval, run


def timing_kiss_check() -> None:
    """Copy this mixed-language workspace into tmp and time `kiss check`."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-check-") as tmp:
        repo = Path(tmp) / "repo"
        shutil.copytree(
            ROOT,
            repo,
            ignore=shutil.ignore_patterns(
                "target",
                ".kiss",
                "_kpop",
                ".cursor",
                ".cursorrules",
                ".testmondata",
                ".malvin",
                ".malvin_home",
                "__pycache__",
                ".pytest_cache",
                "log",
                "log_1",
                "o",
                ".git",
            ),
        )
        env = os.environ.copy()
        env.pop("RUSTFLAGS", None)
        outcome = run(
            "check-complex-tmp",
            [str(KISS), "check", "."],
            repo,
            env,
            expected=None,
            timeout=50,
        )
        assert "Analyzed:" in outcome.stdout, (
            f"kiss check did not finish analysis (rc={outcome.returncode})\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
        emit_eval("kiss_check_elapsed_s", "SMALLER", f"{outcome.elapsed:.4f}")
        print(f"QA PASS: kiss check on complex tmp repo elapsed={outcome.elapsed:.2f}s")


def eval_timing_kiss_check() -> None:
    report_eval(timing_kiss_check)
