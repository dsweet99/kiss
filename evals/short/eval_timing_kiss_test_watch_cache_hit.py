"""Measure warm-watcher no-change client latency for kiss test."""

from __future__ import annotations

import json
import os
import signal
import socket
import struct
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, LinuxProcessObserver, emit_eval, report_eval
from evals.short.eval_timing_kiss_test import write_complex_test_repo


def _session_socket_path(repo: Path) -> str | None:
    session_path = repo / ".kiss" / "watch" / "session.json"
    if not session_path.is_file():
        return None
    try:
        payload = json.loads(session_path.read_text())
    except (OSError, json.JSONDecodeError):
        return None
    socket_path = payload.get("socket")
    return socket_path if isinstance(socket_path, str) and socket_path else None


def _nudge_watcher(socket_path: str, timeout: float = 90.0) -> bool:
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(timeout)
            sock.connect(socket_path)
            body = b"{}"
            sock.sendall(struct.pack(">I", len(body)) + body)
            length_bytes = sock.recv(4)
            if len(length_bytes) != 4:
                return False
            (length,) = struct.unpack(">I", length_bytes)
            if length == 0 or length > 256 * 1024:
                return False
            remaining = length
            while remaining > 0:
                chunk = sock.recv(remaining)
                if not chunk:
                    return False
                remaining -= len(chunk)
            return True
    except OSError:
        return False


def _wait_watch_idle(repo: Path, watcher: subprocess.Popen[str], timeout: float = 90.0) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if watcher.poll() is not None:
            raise AssertionError("watcher exited before becoming idle")
        socket_path = _session_socket_path(repo)
        if socket_path is not None and _nudge_watcher(socket_path):
            return
        time.sleep(0.05)
    raise AssertionError("watcher idle cycle not ready")


def _sample_until(
    process: subprocess.Popen[str],
    observers: list[LinuxProcessObserver],
    timeout: float,
    sample_interval: float = 0.05,
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


def timing_kiss_test_watch_cache_hit() -> None:
    """Warm the watcher, then time a no-change oneshot client request."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-watch-hit-") as tmp:
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
            try:
                _wait_watch_idle(repo, watcher)
                oneshot = subprocess.Popen(
                    [str(KISS), "test", "."],
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
            assert "kiss test: Planning" not in stdout, (
                "oneshot planned locally instead of using warm watcher cache\n"
                f"stdout:\n{stdout}\nstderr:\n{stderr}"
            )
            emit_eval(
                "kiss_test_watch_cache_hit_elapsed_s",
                "SMALLER",
                f"{elapsed:.4f}",
            )
            emit_eval(
                "kiss_test_watch_cache_hit_peak_rss_kib",
                "SMALLER",
                oneshot_obs.observation.peak_rss_kib,
            )


def eval_timing_kiss_test_watch_cache_hit() -> None:
    report_eval(timing_kiss_test_watch_cache_hit)
