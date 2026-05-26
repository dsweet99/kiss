from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path

from ops.cd_analyze_inputs import AnalyzeInputs
from ops.cd_file_coverage import FileCoverage

MAX_COVERAGE_PCT = 100.0


@dataclass(frozen=True)
class DiscrepancyReport:
    repo: Path
    language: str
    n_files: int
    kiss_median_pct: float
    runtime_total_pct: float
    global_gap: float
    file_mae: float
    file_rmse: float
    spearman: float | None
    inflation_rate: float
    blind_spot_rate: float
    pairs: tuple[FileCoverage, ...]


def align_files(
    kiss: dict[Path, float], runtime: dict[Path, float]
) -> list[FileCoverage]:
    common = sorted(set(kiss) & set(runtime))
    return [
        FileCoverage(path=p, kiss_pct=kiss[p], runtime_pct=runtime[p]) for p in common
    ]


def spearman(xs: list[float], ys: list[float]) -> float | None:
    n = len(xs)
    if n < 2:
        return None

    def ranks(vals: list[float]) -> list[float]:
        order = sorted(range(n), key=lambda i: vals[i])
        r = [0.0] * n
        i = 0
        while i < n:
            j = i
            while j + 1 < n and vals[order[j + 1]] == vals[order[i]]:
                j += 1
            avg = (i + j) / 2.0 + 1.0
            for k in range(i, j + 1):
                r[order[k]] = avg
            i = j + 1
        return r

    rx, ry = ranks(xs), ranks(ys)
    d2 = sum((a - b) ** 2 for a, b in zip(rx, ry))
    return 1.0 - (6.0 * d2) / (n * (n * n - 1))


def analyze(inputs: AnalyzeInputs) -> DiscrepancyReport:
    pairs = align_files(inputs.kiss_map, inputs.runtime_map)
    if not pairs:
        raise RuntimeError(
            f"no overlapping files between kiss and runtime in {inputs.repo}"
        )

    diffs = [p.kiss_pct - p.runtime_pct for p in pairs]
    abs_diffs = [abs(d) for d in diffs]
    sq_diffs = [d * d for d in diffs]
    n = len(pairs)
    global_gap = abs(inputs.kiss_median - inputs.runtime_total)
    inflation = sum(1 for d in diffs if d >= 20) / n
    blind = sum(1 for d in diffs if d <= -20) / n
    sp = spearman([p.kiss_pct for p in pairs], [p.runtime_pct for p in pairs])

    file_mae = sum(abs_diffs) / n
    file_rmse = math.sqrt(sum(sq_diffs) / n) / MAX_COVERAGE_PCT

    return DiscrepancyReport(
        repo=inputs.repo.resolve(),
        language=inputs.language,
        n_files=n,
        kiss_median_pct=inputs.kiss_median,
        runtime_total_pct=inputs.runtime_total,
        global_gap=global_gap,
        file_mae=file_mae,
        file_rmse=file_rmse,
        spearman=sp,
        inflation_rate=inflation,
        blind_spot_rate=blind,
        pairs=tuple(pairs),
    )
