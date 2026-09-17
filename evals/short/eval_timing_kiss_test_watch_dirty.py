"""Measure warm-watcher dirty-path oneshot latency after a source change."""

from __future__ import annotations

import os
import signal
import subprocess
import tempfile
from pathlib import Path

from evals._harness import KISS, LinuxProcessObserver, emit_eval, report_eval
from evals.short.eval_timing_kiss_test import write_complex_test_repo
from evals.short.eval_timing_kiss_test_watch_cache_hit import (
    _sample_until,
    _wait_watch_idle,
)


def timing_kiss_test_watch_dirty() -> None:
    """Warm the watcher, mutate a source file, then time the oneshot re-run."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-watch-dirty-") as tmp:
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
                [str(KISS), "test", "--watch", "--lang", "python", "."],
                cwd=repo,
                env=env,
                text=True,
                stdout=watch_out,
                stderr=watch_err,
            )
            watcher_obs = LinuxProcessObserver(watcher.pid)
            try:
                _wait_watch_idle(repo, watcher)
                (repo / "pkg/app.py").write_text(
                    "def alpha():\n"
                    "    return 'alpha'\n"
                    "\n"
                    "def add(a, b):\n"
                    "    return a + b + 0\n",
                )
                oneshot = subprocess.Popen(
                    [str(KISS), "test", "--lang", "python", "."],
                    cwd=repo,
                    env=env,
                    text=True,
                    stdout=test_out,
                    stderr=test_err,
                )
                oneshot_obs = LinuxProcessObserver(oneshot.pid)
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
            assert "passed" in stdout.lower() or "PASS" in stdout, (
                "dirty watch oneshot must echo results\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
            emit_eval(
                "kiss_test_watch_dirty_elapsed_s",
                "SMALLER",
                f"{elapsed:.4f}",
            )
            emit_eval(
                "kiss_test_watch_dirty_peak_rss_kib",
                "SMALLER",
                oneshot_obs.observation.peak_rss_kib,
            )


def eval_timing_kiss_test_watch_dirty() -> None:
    report_eval(timing_kiss_test_watch_dirty)
