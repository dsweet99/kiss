from __future__ import annotations

from pathlib import Path

from ops.cd_runtime_io import (
    KISS_ROOT,
    _bounded_diagnostics,
    _load_json,
    _run_to_temp_files,
    _scan_stdout_prefix,
    _unlink_paths,
)


def _kiss_map_from_paths(stdout_path: Path, stderr_path: Path) -> dict[Path, float] | None:
    try:
        if stdout_path.stat().st_size == 0:
            return None
        raw = _load_json(stdout_path)
    finally:
        _unlink_paths(stdout_path, stderr_path)
    assert isinstance(raw, dict)
    return {Path(k).resolve(): float(v) for k, v in raw.items()}


def _try_kiss_coverage_cmd(cmd: list[str], cwd: Path) -> dict[Path, float] | None:
    try:
        code, stdout_path, stderr_path = _run_to_temp_files(cmd, cwd=cwd)
    except FileNotFoundError:
        return None
    if code != 0:
        _unlink_paths(stdout_path, stderr_path)
        return None
    return _kiss_map_from_paths(stdout_path, stderr_path)


def kiss_per_file(repo: Path) -> dict[Path, float]:
    repo = repo.resolve()
    binary = KISS_ROOT / "target" / "debug" / "kiss-coverage-map"
    for cmd, cwd in (
        ([str(binary), "."], repo),
        (
            [
                "cargo",
                "run",
                "-q",
                "--manifest-path",
                str(KISS_ROOT / "Cargo.toml"),
                "--bin",
                "kiss-coverage-map",
                "--",
                ".",
            ],
            repo,
        ),
    ):
        result = _try_kiss_coverage_cmd(cmd, cwd)
        if result is not None:
            return result
    raise RuntimeError(f"could not obtain kiss per-file coverage for {repo}")


def _parse_inv_p50_from_stats(stdout_path: Path) -> int:
    line = _scan_stdout_prefix(stdout_path, "inv_test_coverage")
    if line is None:
        return 0
    parts = line.split()
    if len(parts) < 3:
        return 0
    return int(parts[2])


def kiss_summary_median(repo: Path) -> float:
    code, stdout_path, stderr_path = _run_to_temp_files(
        ["kiss", "stats", str(repo)], cwd=repo
    )
    try:
        if code != 0:
            raise RuntimeError(
                f"command failed ({code}): kiss stats {repo}\n"
                f"{_bounded_diagnostics(stdout_path, stderr_path)}"
            )
        return 100.0 - _parse_inv_p50_from_stats(stdout_path)
    finally:
        _unlink_paths(stdout_path, stderr_path)
