from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from ops.cd_analyze_inputs import AnalyzeInputs
from ops.cd_discrepancy_report import DiscrepancyReport, analyze
from ops.cd_runtime import kiss_per_file, kiss_summary_median


@dataclass(frozen=True)
class RuntimeCoverage:
    per_file: dict[Path, float]
    total_pct: float


def analyze_discrepancy(
    repo: Path, language: str, runtime: RuntimeCoverage
) -> DiscrepancyReport:
    return analyze(
        AnalyzeInputs(
            repo=repo,
            language=language,
            runtime_map=runtime.per_file,
            runtime_total=runtime.total_pct,
            kiss_map=kiss_per_file(repo, language=language),
            kiss_median=kiss_summary_median(repo),
        )
    )
