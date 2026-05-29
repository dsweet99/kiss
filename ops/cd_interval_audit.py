#!/usr/bin/env python3
"""Audit idea #8: runtime should fall between attested and optimistic static bounds."""

from __future__ import annotations

import math
import sys
from dataclasses import dataclass
from pathlib import Path

from ops.cd_python_source import infer_slipcover_source
from ops.cd_runtime import slipcover_per_file
from ops.cd_runtime_io import KISS_ROOT, _load_json, _run_to_temp_files, _unlink_paths


@dataclass(frozen=True)
class IntervalAudit:
    repo: Path
    n_files: int
    outside_rate: float
    wide_interval_rate: float
    shipped_rmse: float
    attested_rmse: float
    optimistic_rmse: float
    outside_inflated: int
    outside_blind: int
    outside_neutral: int


def _kiss_map(repo: Path, *, language: str, bound_flag: str | None) -> dict[Path, float]:
    binary = KISS_ROOT / "target" / "release" / "kiss-coverage-map"
    if not binary.is_file():
        binary = KISS_ROOT / "target" / "debug" / "kiss-coverage-map"
    if not binary.is_file():
        raise RuntimeError("kiss-coverage-map binary missing; run cargo build --bin kiss-coverage-map")
    cmd = [str(binary), "--python" if language == "python" else "--rust", "."]
    if bound_flag:
        cmd.insert(1, bound_flag)
    code, stdout_path, stderr_path = _run_to_temp_files(cmd, cwd=repo)
    try:
        if code != 0:
            raise RuntimeError(f"kiss-coverage-map failed ({code}) in {repo}")
        raw = _load_json(stdout_path)
    finally:
        _unlink_paths(stdout_path, stderr_path)
    return {Path(k).resolve(): float(v) for k, v in raw.items()}


def _rmse(pairs: list[tuple[float, float]]) -> float:
    if not pairs:
        return float("nan")
    sq = sum((k - r) ** 2 for k, r in pairs)
    return math.sqrt(sq / len(pairs)) / 100.0


def _outside_gap_bucket(gap: float) -> str:
    if gap >= 20:
        return "inflated"
    if gap <= -20:
        return "blind"
    return "neutral"


def _python_audit_inputs(repo: Path) -> dict[str, object]:
    source = infer_slipcover_source(repo)
    runtime, _runtime_total = slipcover_per_file(repo, [], source=source)
    shipped = _kiss_map(repo, language="python", bound_flag=None)
    attested = _kiss_map(repo, language="python", bound_flag="--attested")
    optimistic = _kiss_map(repo, language="python", bound_flag="--optimistic")
    common = sorted(set(runtime) & set(shipped) & set(attested) & set(optimistic))
    return {
        "common": common,
        "runtime": runtime,
        "shipped": shipped,
        "attested": attested,
        "optimistic": optimistic,
    }


def _record_outside_gap(stats: dict[str, int], gap: float) -> None:
    key = {
        "inflated": "outside_inflated",
        "blind": "outside_blind",
        "neutral": "outside_neutral",
    }[_outside_gap_bucket(gap)]
    stats[key] += 1


def _interval_count_stats(
    common: list[Path],
    bundled: tuple[
        dict[Path, float],
        dict[Path, float],
        dict[Path, float],
        dict[Path, float],
    ],
) -> dict[str, int]:
    runtime, shipped, attested, optimistic = bundled
    stats = {
        "outside": 0,
        "wide": 0,
        "outside_inflated": 0,
        "outside_blind": 0,
        "outside_neutral": 0,
    }
    for path in common:
        lo = min(attested[path], optimistic[path])
        hi = max(attested[path], optimistic[path])
        rt = runtime[path]
        if rt < lo - 1e-9 or rt > hi + 1e-9:
            stats["outside"] += 1
            _record_outside_gap(stats, shipped[path] - rt)
        if hi - lo > 50:
            stats["wide"] += 1
    return stats


def audit_python_repo(repo: Path) -> IntervalAudit:
    repo = repo.resolve()
    inputs = _python_audit_inputs(repo)
    common = inputs["common"]
    runtime = inputs["runtime"]
    shipped = inputs["shipped"]
    attested = inputs["attested"]
    optimistic = inputs["optimistic"]
    stats = _interval_count_stats(
        common,
        (runtime, shipped, attested, optimistic),
    )
    n = len(common)
    return IntervalAudit(
        repo=repo,
        n_files=n,
        outside_rate=stats["outside"] / n if n else float("nan"),
        wide_interval_rate=stats["wide"] / n if n else float("nan"),
        shipped_rmse=_rmse([(shipped[p], runtime[p]) for p in common]),
        attested_rmse=_rmse([(attested[p], runtime[p]) for p in common]),
        optimistic_rmse=_rmse([(optimistic[p], runtime[p]) for p in common]),
        outside_inflated=stats["outside_inflated"],
        outside_blind=stats["outside_blind"],
        outside_neutral=stats["outside_neutral"],
    )


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: cd_interval_audit.py REPO [REPO...]", file=sys.stderr)
        return 2
    for row in (audit_python_repo(Path(arg)) for arg in argv[1:]):
        print(
            f"{row.repo.name}: n={row.n_files} outside={row.outside_rate:.3f} "
            f"wide_interval={row.wide_interval_rate:.3f} "
            f"rmse shipped/attested/optimistic="
            f"{row.shipped_rmse:.3f}/{row.attested_rmse:.3f}/{row.optimistic_rmse:.3f} "
            f"outside_by_gap inflated/blind/neutral="
            f"{row.outside_inflated}/{row.outside_blind}/{row.outside_neutral}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
