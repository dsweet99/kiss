"""Time kiss test against a just-started watcher on the mixed-language tmp repo."""

from __future__ import annotations

import os
import signal
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, LinuxProcessObserver, emit_eval, report_eval
from evals.eval_timing_kiss_test import write_complex_test_repo


def _sample_until(
    process: subprocess.Popen[str],
    observers: list[LinuxProcessObserver],
    timeout: float,
    sample_interval: float = 0.1,
) -> float:
    started = time.monotonic()
    deadline = started + timeout
    while process.poll() is None:
        for observer in observers:
            observer.sample()
        if time.monotonic() > deadline:
            process.kill()
            process.wait()
            raise subprocess.TimeoutExpired(process.args, timeout)
        try:
            process.wait(timeout=sample_interval)
        except subprocess.TimeoutExpired:
            continue
    for observer in observers:
        observer.sample()
    return time.monotonic() - started


def timing_kiss_test_watch() -> None:
    """Start the watcher, immediately run `kiss test`, and measure both."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-watch-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_complex_test_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)
        with (
            tempfile.TemporaryFile("w+t") as watch_out,
            tempfile.TemporaryFile("w+t") as watch_err,
            tempfile.TemporaryFile("w+t") as test_out,
            tempfile.TemporaryFile("w+t") as test_err,
        ):
            watcher = subprocess.Popen(
                [str(KISS), "test", "--watch", "."],
                cwd=repo,
                env=env,
                text=True,
                stdout=watch_out,
                stderr=watch_err,
            )
            watcher_obs = LinuxProcessObserver(watcher.pid)
            oneshot = subprocess.Popen(
                [str(KISS), "test", "."],
                cwd=repo,
                env=env,
                text=True,
                stdout=test_out,
                stderr=test_err,
            )
            oneshot_obs = LinuxProcessObserver(oneshot.pid)
            try:
                elapsed = _sample_until(
                    oneshot,
                    [watcher_obs, oneshot_obs],
                    timeout=50,
                )
            finally:
                if watcher.poll() is None:
                    watcher.send_signal(signal.SIGKILL)
                    watcher.wait(timeout=5)
            test_out.seek(0)
            test_err.seek(0)
            stdout = test_out.read()
            stderr = test_err.read()
            assert oneshot.returncode == 0, (
                f"kiss test failed rc={oneshot.returncode}\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
            assert "PASS" in stdout or "passed" in stdout, (
                f"kiss test did not report results\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
            emit_eval("kiss_test_elapsed_s", "SMALLER", f"{elapsed:.4f}")
            emit_eval(
                "kiss_test_peak_rss_kib",
                "SMALLER",
                oneshot_obs.observation.peak_rss_kib,
            )
            emit_eval(
                "watcher_peak_rss_kib",
                "SMALLER",
                watcher_obs.observation.peak_rss_kib,
            )


def eval_timing_kiss_test_watch() -> None:
    report_eval(timing_kiss_test_watch)
