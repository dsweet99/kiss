#!/usr/bin/env python3
"""Compare kiss static test-name coverage vs runtime line coverage."""

from __future__ import annotations

from ops.cd_analyze import RuntimeCoverage, analyze_discrepancy as analyze
from ops.cd_click import cli
from ops.cd_cli import main, rust_cmd  # registers `rust` and `python` on cli
from ops.cd_discrepancy_report import DiscrepancyReport, align_files, spearman
from ops.cd_file_coverage import FileCoverage
from ops.cd_python_run import (
    PythonCoverageRun,
    python,
    python_cmd,
    run_python_coverage_discrepancy,
)
from ops.cd_report_io import (
    emit_report,
    print_detailed_report,
    print_report,
    write_report_json,
)
from ops.cd_runtime import (
    kiss_per_file,
    kiss_summary_median,
    llvm_cov_per_file,
    run,
    slipcover_per_file,
)

__all__ = [
    "DiscrepancyReport",
    "FileCoverage",
    "PythonCoverageRun",
    "RuntimeCoverage",
    "align_files",
    "analyze",
    "cli",
    "emit_report",
    "kiss_per_file",
    "kiss_summary_median",
    "llvm_cov_per_file",
    "main",
    "print_detailed_report",
    "print_report",
    "python",
    "python_cmd",
    "run",
    "run_python_coverage_discrepancy",
    "rust_cmd",
    "slipcover_per_file",
    "spearman",
    "write_report_json",
]

if __name__ == "__main__":
    main()
