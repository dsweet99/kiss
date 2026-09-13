"""Measure kiss test --retry-bad selective re-run wall time on a tiny Python repo."""

from __future__ import annotations

import os
import tempfile
from pathlib import Path

from evals._harness import KISS, commit_fixture_baseline, emit_eval, report_eval, run


def _write_retry_bad_repo(repo: Path) -> None:
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "[test]\n"
        "test_coverage_threshold = 0\n"
        "orphan_detection = false\n"
        "num_jobs = 1\n"
        "[python]\n"
        "[rust]\n"
    )
    (repo / ".gitignore").write_text(".kiss/\n__pycache__/\n")
    (repo / "lib.py").write_text("VALUE = 0\n")
    (repo / "test_lib.py").write_text(
        "import lib\n"
        "\n"
        "\n"
        "def test_ok():\n"
        "    assert True\n"
        "\n"
        "\n"
        "def test_flip():\n"
        "    assert lib.VALUE == 1\n"
    )
    commit_fixture_baseline(repo)


def timing_kiss_test_retry_bad() -> None:
    """Seed a FAIL, then time `--retry-bad` after flipping the fixture to PASS."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-retry-bad-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        _write_retry_bad_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)

        first = run(
            "kiss-test-seed-fail",
            [str(KISS), "test", "--lang", "python", "."],
            repo,
            env,
            expected=1,
            timeout=40,
        )
        assert "FAIL" in first.stdout or "failed" in first.stdout.lower(), first.stdout

        (repo / "lib.py").write_text("VALUE = 1\n")
        retry = run(
            "kiss-test-retry-bad",
            [str(KISS), "test", "--lang", "python", ".", "--retry-bad"],
            repo,
            env,
            expected=0,
            timeout=40,
        )
        assert "PASS" in retry.stdout or "passed" in retry.stdout.lower(), retry.stdout
        emit_eval(
            "kiss_test_retry_bad_elapsed_s",
            "SMALLER",
            f"{retry.elapsed:.4f}",
        )


def eval_timing_kiss_test_retry_bad() -> None:
    report_eval(timing_kiss_test_retry_bad)
