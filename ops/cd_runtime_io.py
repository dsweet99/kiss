from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

KISS_ROOT = Path(__file__).resolve().parents[1]
_MAX_DIAG_BYTES = 65536
_MAX_RUN_BYTES = 1_048_576
_MAX_JSON_BYTES = 64 * 1024 * 1024


def _bounded_text(path: Path, max_bytes: int = _MAX_DIAG_BYTES) -> str:
    if not path.exists():
        return ""
    size = path.stat().st_size
    if size == 0:
        return ""
    with path.open("rb") as handle:
        if size > max_bytes:
            handle.seek(-max_bytes, 2)
        return handle.read(max_bytes).decode("utf-8", errors="replace")


def _bounded_diagnostics(stdout_path: Path, stderr_path: Path) -> str:
    parts: list[str] = []
    for label, path in (("stderr", stderr_path), ("stdout", stdout_path)):
        text = _bounded_text(path)
        if text:
            parts.append(f"{label}:\n{text}")
    return "\n".join(parts) or "(no output)"


def _unlink_paths(*paths: Path) -> None:
    for path in paths:
        path.unlink(missing_ok=True)


def _subprocess_env() -> dict[str, str]:
    env = os.environ.copy()
    env.setdefault("NEXTEST_TEST_THREADS", "2")
    env.setdefault("CARGO_BUILD_JOBS", "2")
    return env


def _run_to_temp_files(
    cmd: list[str], *, cwd: Path | None = None
) -> tuple[int, Path, Path]:
    with tempfile.NamedTemporaryFile(
        mode="w+", suffix=".stdout", delete=False, encoding="utf-8"
    ) as out_tmp:
        stdout_path = Path(out_tmp.name)
    with tempfile.NamedTemporaryFile(
        mode="w+", suffix=".stderr", delete=False, encoding="utf-8"
    ) as err_tmp:
        stderr_path = Path(err_tmp.name)
    with stdout_path.open("w", encoding="utf-8") as stdout_f, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr_f:
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            text=True,
            capture_output=False,
            stdout=stdout_f,
            stderr=stderr_f,
            check=False,
            env=_subprocess_env(),
        )
    return proc.returncode, stdout_path, stderr_path


def _load_json(path: Path, *, max_bytes: int = _MAX_JSON_BYTES) -> object:
    size = path.stat().st_size
    if size > max_bytes:
        raise RuntimeError(
            f"json artifact too large ({size} bytes > {max_bytes}): {path}"
        )
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def _read_bounded_file(path: Path, *, max_bytes: int = _MAX_RUN_BYTES) -> str:
    size = path.stat().st_size
    if size > max_bytes:
        raise RuntimeError(
            f"command output too large ({size} bytes > {max_bytes}): "
            f"use an artifact file instead of run()"
        )
    return path.read_text()


def _run_check_only(cmd: list[str], *, cwd: Path | None = None) -> int:
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
        env=_subprocess_env(),
    )
    return proc.returncode


def _scan_stdout_prefix(path: Path, prefix: str) -> str | None:
    with path.open(encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if line.startswith(prefix):
                return line
    return None


def run(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    check: bool = True,
    max_bytes: int = _MAX_RUN_BYTES,
) -> str:
    code, stdout_path, stderr_path = _run_to_temp_files(cmd, cwd=cwd)
    try:
        if check and code != 0:
            raise RuntimeError(
                f"command failed ({code}): {' '.join(cmd)}\n"
                f"{_bounded_diagnostics(stdout_path, stderr_path)}"
            )
        return _read_bounded_file(stdout_path, max_bytes=max_bytes)
    finally:
        _unlink_paths(stdout_path, stderr_path)
