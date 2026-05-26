from __future__ import annotations

import tempfile
from dataclasses import dataclass
from pathlib import Path

from ops.cd_runtime_io import (
    _bounded_diagnostics,
    _load_json,
    _run_to_temp_files,
    _unlink_paths,
)


@dataclass(frozen=True)
class _SlipcoverRun:
    repo: Path
    pytest_args: list[str]
    out_path: str
    source: str | None


def _run_slipcover(spec: _SlipcoverRun) -> None:
    cmd = ["slipcover", "--json", "--out", spec.out_path]
    if spec.source:
        cmd.extend(["--source", spec.source])
    cmd.extend(["-m", "pytest", *spec.pytest_args])
    code, stdout_path, stderr_path = _run_to_temp_files(cmd, cwd=spec.repo)
    try:
        out = Path(spec.out_path)
        if not out.exists() or out.stat().st_size == 0:
            raise RuntimeError(
                f"slipcover failed ({code}): {_bounded_diagnostics(stdout_path, stderr_path)}"
            )
    finally:
        _unlink_paths(stdout_path, stderr_path)


def _parse_slipcover_json(
    repo: Path, data: dict
) -> tuple[dict[Path, float], float]:
    per_file: dict[Path, float] = {}
    total_lines = covered_lines = 0
    for rel, info in data.get("files", {}).items():
        path = (repo / rel).resolve()
        summ = info.get("summary", {})
        n_cov = int(summ.get("covered_lines", 0))
        n_miss = int(summ.get("missing_lines", 0))
        total = n_cov + n_miss
        pct = 100.0 * n_cov / total if total else 0.0
        if total > 0:
            per_file[path] = pct
            total_lines += total
            covered_lines += n_cov
    summary = data.get("summary", {})
    total_pct = float(summary.get("percent_covered", 0.0))
    if not total_pct and total_lines:
        total_pct = 100.0 * covered_lines / total_lines
    return per_file, total_pct


def slipcover_per_file(
    repo: Path, pytest_args: list[str], *, source: str | None = None
) -> tuple[dict[Path, float], float]:
    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        out_path = tmp.name
    try:
        _run_slipcover(_SlipcoverRun(repo, pytest_args, out_path, source))
        data = _load_json(Path(out_path))
    finally:
        Path(out_path).unlink(missing_ok=True)
    assert isinstance(data, dict)
    return _parse_slipcover_json(repo, data)
