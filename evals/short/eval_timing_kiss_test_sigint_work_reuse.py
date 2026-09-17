"""Measure post-SIGINT restart work reuse via cached PASS lines."""

from __future__ import annotations

import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, commit_fixture_baseline, emit_eval, report_eval, run


def _write_python_fast_slow_repo(repo: Path) -> None:
    (repo / ".kissconfig").write_text(
        "[global]\n"
        "duplication_enabled = false\n"
        "[test]\n"
        "test_coverage_threshold = 0\n"
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
        "    time.sleep(8)\n"
        "    assert True\n"
    )
    commit_fixture_baseline(repo)


def _interrupt_after_first_pass(repo: Path, env: dict[str, str], timeout: float) -> None:
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
        while process.poll() is None and time.monotonic() < deadline:
            stdout_file.seek(0)
            snap = stdout_file.read()
            if "PASS: test_lib.py::test_fast" in snap or "PASS: test_fast" in snap:
                saw_pass = True
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
                "no test_fast PASS line before SIGINT window closed\n"
                f"rc={process.returncode}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        try:
            process.wait(timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise


def timing_kiss_test_sigint_work_reuse() -> None:
    """SIGINT after first PASS, then count cached PASS lines on restart."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-sir-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        _write_python_fast_slow_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)

        _interrupt_after_first_pass(repo, env, timeout=50)
        restart = run(
            "kiss-test-post-interrupt-reuse",
            [str(KISS), "test", "--lang", "python", "."],
            repo,
            env,
            expected=0,
            timeout=50,
        )
        cached_lines = sum(
            1
            for line in restart.stdout.splitlines()
            if "PASS (cached)" in line
        )
        assert cached_lines >= 1, (
            "restart must reuse at least one PASS from before SIGINT\n"
            f"stdout:\n{restart.stdout}\nstderr:\n{restart.stderr}"
        )
        emit_eval(
            "kiss_test_sigint_restart_cached_pass_lines",
            "LARGER",
            cached_lines,
        )
        emit_eval(
            "kiss_test_sigint_restart_reuse_elapsed_s",
            "SMALLER",
            f"{restart.elapsed:.4f}",
        )


def eval_timing_kiss_test_sigint_work_reuse() -> None:
    report_eval(timing_kiss_test_sigint_work_reuse)
