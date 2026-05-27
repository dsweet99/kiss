from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from ops.cd_analyze import RuntimeCoverage, analyze_discrepancy as analyze
from ops.cd_report_io import emit_report
from ops.cd_runtime import slipcover_per_file


@dataclass(frozen=True)
class PythonCoverageRun:
    repo: Path
    slipcover_source: str | None
    pytest_args: tuple[str, ...]
    detailed: bool
    report_out: Path | None


def run_python_coverage_discrepancy(run: PythonCoverageRun) -> None:
    runtime_map, runtime_total = slipcover_per_file(
        run.repo, list(run.pytest_args), source=run.slipcover_source
    )
    emit_report(
        analyze(
            run.repo,
            "python",
            RuntimeCoverage(runtime_map, runtime_total),
        ),
        detailed=run.detailed,
        report_out=run.report_out,
    )


def python_cmd(run: PythonCoverageRun) -> None:
    run_python_coverage_discrepancy(run)


python = python_cmd
