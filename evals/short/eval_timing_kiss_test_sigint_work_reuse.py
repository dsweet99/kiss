"""Measure post-SIGINT restart work reuse via reused (cache-hit) tests."""

from __future__ import annotations

import os
import re
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import (
    KISS,
    commit_fixture_baseline,
    emit_eval,
    python_records_dir,
    report_eval,
    run,
)

SLOW_TEST_SECONDS = 8


def _write_python_fast_slow_repo(repo: Path) -> None:
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "[test]\n"
        "orphan_detection = false\n"
        "num_jobs = 1\n"
        "[test.max_unit_test_seconds]\n"
        '"*" = 30\n'
        "[python]\n"
        "[rust]\n"
    )
    (repo / ".gitignore").write_text(".kiss/\n__pycache__/\n")
    (repo / "lib.py").write_text("VALUE = 1\n")
    (repo / "test_lib.py").write_text(
        "import time\n"
        "\n"
        "\n"
        "def test_fast():\n"
        "    assert True\n"
        "\n"
        "\n"
        "def test_slow():\n"
        f"    time.sleep({SLOW_TEST_SECONDS})\n"
        "    assert True\n"
    )
    commit_fixture_baseline(repo)


def _interrupt_after_first_pass(repo: Path, env: dict[str, str], timeout: float) -> float:
    started = time.monotonic()
    with (
        tempfile.TemporaryFile("w+t") as stdout_file,
        tempfile.TemporaryFile("w+t") as stderr_file,
    ):
        process = subprocess.Popen(
            [str(KISS), "test", "--lang", "python", "."],
            cwd=repo,
            env=env,
            text=True,
            stdout=stdout_file,
            stderr=stderr_file,
            start_new_session=True,
        )
        deadline = started + timeout
        saw_pass = False
        interrupted_after = 0.0
        while process.poll() is None and time.monotonic() < deadline:
            if any(python_records_dir(repo).glob("*.json")):
                saw_pass = True
                interrupted_after = time.monotonic() - started
                os.killpg(os.getpgid(process.pid), signal.SIGINT)
                break
            time.sleep(0.05)
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read()
        stderr = stderr_file.read()
        if not saw_pass:
            if process.poll() is None:
                process.kill()
                process.wait()
            raise AssertionError(
                "no test record written before SIGINT window closed\n"
                f"rc={process.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        try:
            process.wait(timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
        return interrupted_after


def _reused_tests(stdout: str) -> int:
    """Passed tests minus executed ones; a cached PASS prints no result line."""
    match = re.search(r"(\d+) passed", stdout)
    passed = int(match.group(1)) if match else 0
    executed = sum(
        1 for line in stdout.splitlines() if line.startswith(("PASS", "FAIL", "TIMEOUT"))
    )
    return passed - executed


def timing_kiss_test_sigint_work_reuse() -> None:
    """SIGINT after the first test record lands, then count reused tests on restart."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-sir-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        _write_python_fast_slow_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)

        interrupted_after = _interrupt_after_first_pass(repo, env, timeout=50)
        assert interrupted_after < SLOW_TEST_SECONDS, (
            "test_fast's record must land while test_slow is still running; "
            f"first record appeared after {interrupted_after:.2f}s"
        )
        restart = run(
            "kiss-test-post-interrupt-reuse",
            [str(KISS), "test", "--lang", "python", "."],
            repo,
            env,
            expected=0,
            timeout=50,
        )
        reused = _reused_tests(restart.stdout)
        assert reused >= 1, (
            "restart must reuse at least one PASS from before SIGINT\n"
            f"stdout:\n{restart.stdout}\nstderr:\n{restart.stderr}"
        )
        emit_eval(
            "kiss_test_sigint_restart_reused_tests",
            "LARGER",
            reused,
        )
        emit_eval(
            "kiss_test_sigint_restart_reuse_elapsed_s",
            "SMALLER",
            f"{restart.elapsed:.4f}",
        )


def eval_timing_kiss_test_sigint_work_reuse() -> None:
    report_eval(timing_kiss_test_sigint_work_reuse)
