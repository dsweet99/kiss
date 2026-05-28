from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from ops.cd_analyze_inputs import AnalyzeInputs
from ops.cd_discrepancy_report import DiscrepancyReport, analyze
from ops.cd_include_rollup import build_rust_include_edges, rollup_inc_coverage
from ops.cd_runtime import kiss_per_file, kiss_summary_median


@dataclass(frozen=True)
class RuntimeCoverage:
    per_file: dict[Path, float]
    total_pct: float


def _filter_kiss_to_slipcover_tree(
    kiss_map: dict[Path, float], repo: Path, source: str | None
) -> dict[Path, float]:
    if not source:
        return kiss_map
    root = (repo / source).resolve()
    return {
        path: pct
        for path, pct in kiss_map.items()
        if path == root or root in path.parents
    }


def analyze_discrepancy(
    repo: Path, language: str, runtime: RuntimeCoverage, *, slipcover_source: str | None = None
) -> DiscrepancyReport:
    kiss_map = kiss_per_file(repo, language=language)
    runtime_map = runtime.per_file
    if language == "python":
        kiss_map = _filter_kiss_to_slipcover_tree(kiss_map, repo, slipcover_source)
    if language == "rust":
        include_edges = build_rust_include_edges(repo)
        kiss_map = rollup_inc_coverage(kiss_map, include_edges)
        runtime_map = rollup_inc_coverage(runtime_map, include_edges)
    return analyze(
        AnalyzeInputs(
            repo=repo,
            language=language,
            runtime_map=runtime_map,
            runtime_total=runtime.total_pct,
            kiss_map=kiss_map,
            kiss_median=kiss_summary_median(repo),
        )
    )
