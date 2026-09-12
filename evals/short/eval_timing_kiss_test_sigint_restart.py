"""Measure kiss test SIGINT exit latency and post-interrupt restart cost."""

from __future__ import annotations

import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, emit_eval, report_eval, run
from evals.short.eval_timing_kiss_test import write_complex_test_repo


def _interrupt_and_measure_exit(
    repo: Path,
    env: dict[str, str],
    *,
    signal_after: float,
    timeout: float,
) -> float:
    started = time.monotonic()
    with (
        tempfile.TemporaryFile("w+t") as stdout_file,
        tempfile.TemporaryFile("w+t") as stderr_file,
    ):
        process = subprocess.Popen(
            [str(KISS), "test", "."],
            cwd=repo,
            env=env,
            text=True,
            stdout=stdout_file,
            stderr=stderr_file,
            start_new_session=True,
        )
        deadline = started + timeout
        signaled_at: float | None = None
        while process.poll() is None and time.monotonic() < deadline:
            if signaled_at is None and time.monotonic() - started >= signal_after:
                os.killpg(os.getpgid(process.pid), signal.SIGINT)
                signaled_at = time.monotonic()
                break
            time.sleep(0.02)
        if signaled_at is None:
            if process.poll() is None:
                process.kill()
                process.wait()
            raise AssertionError(
                "kiss test exited or timed out before SIGINT could be delivered"
            )
        try:
            process.wait(timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
            raise
        exit_latency = time.monotonic() - signaled_at
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read()
        stderr = stderr_file.read()
    assert process.returncode is not None, "interrupted process missing return code"
    # Validity only: process must have stopped after SIGINT.
    assert process.poll() is not None, (
        f"process still running after SIGINT\nstdout:\n{stdout}\nstderr:\n{stderr}"
    )
    return exit_latency


def timing_kiss_test_sigint_restart() -> None:
    """SIGINT a cold run, then time a full restart on the same fixture."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-sigint-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_complex_test_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)

        exit_latency = _interrupt_and_measure_exit(
            repo,
            env,
            signal_after=1.0,
            timeout=50,
        )
        restart = run(
            "kiss-test-post-interrupt",
            [str(KISS), "test", "."],
            repo,
            env,
            expected=0,
            timeout=50,
        )
        emit_eval(
            "kiss_test_sigint_exit_latency_s",
            "SMALLER",
            f"{exit_latency:.4f}",
        )
        emit_eval(
            "kiss_test_post_interrupt_elapsed_s",
            "SMALLER",
            f"{restart.elapsed:.4f}",
        )


def eval_timing_kiss_test_sigint_restart() -> None:
    report_eval(timing_kiss_test_sigint_restart)
