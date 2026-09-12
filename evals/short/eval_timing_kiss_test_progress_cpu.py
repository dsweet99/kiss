"""Measure kiss test progress-log gaps and CPU utilization versus -j budget."""

from __future__ import annotations

import os
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, LinuxProcessObserver, emit_eval, report_eval
from evals.short.eval_timing_kiss_test import write_complex_test_repo

JOBS = 2


def _max_progress_gap_s(progress_times: list[float]) -> float:
    if len(progress_times) < 2:
        return float(progress_times[0]) if progress_times else 0.0
    return max(
        later - earlier
        for earlier, later in zip(progress_times, progress_times[1:], strict=False)
    )


def timing_kiss_test_progress_cpu() -> None:
    """Run kiss test -j N while sampling CPU and progress-line gaps."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-progress-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_complex_test_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)

        started = time.monotonic()
        process = subprocess.Popen(
            [str(KISS), "test", "-j", str(JOBS), "."],
            cwd=repo,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=1,
        )
        assert process.stdout is not None
        observer = LinuxProcessObserver(process.pid)
        progress_times: list[float] = []
        stdout_chunks: list[str] = []
        deadline = started + 50.0
        while True:
            observer.sample()
            if time.monotonic() > deadline:
                process.kill()
                process.wait()
                raise subprocess.TimeoutExpired(process.args, 50)
            line = process.stdout.readline()
            if line:
                stdout_chunks.append(line)
                if line.startswith("kiss test:"):
                    progress_times.append(time.monotonic())
                continue
            if process.poll() is not None:
                break
            time.sleep(0.02)
        observer.sample()
        # Drain any remaining buffered stdout after exit.
        remainder = process.stdout.read()
        if remainder:
            stdout_chunks.append(remainder)
            for line in remainder.splitlines():
                if line.startswith("kiss test:"):
                    progress_times.append(time.monotonic())
        stderr = process.stderr.read() if process.stderr is not None else ""
        stdout = "".join(stdout_chunks)
        elapsed = time.monotonic() - started
        assert process.returncode == 0, (
            f"kiss test failed rc={process.returncode}\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
        assert progress_times, (
            "no kiss test: progress lines observed\n"
            f"stdout:\n{stdout}\nstderr:\n{stderr}"
        )
        max_gap = _max_progress_gap_s(progress_times)
        cpu_seconds = observer.observation.sampled_cpu_seconds
        utilization = 0.0 if elapsed <= 0 else cpu_seconds / (elapsed * JOBS)
        emit_eval("kiss_test_max_progress_gap_s", "SMALLER", f"{max_gap:.4f}")
        emit_eval("kiss_test_cpu_utilization", "LARGER", f"{utilization:.4f}")


def eval_timing_kiss_test_progress_cpu() -> None:
    report_eval(timing_kiss_test_progress_cpu)
