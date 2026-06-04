"""Compare kiss static test-reference coverage to runtime line coverage."""

from __future__ import annotations

import statistics
from pathlib import Path
from typing import NamedTuple

from scipy.stats import spearmanr

from python.coverage_collect import run_true_coverage
from python.coverage_kiss import run_kiss_check_all
from python.coverage_stats import percentile


class CoverageComparison(NamedTuple):
    paths: list[str]
    true_vals: list[float]
    kiss_vals: list[float]
    errors: list[float]


def kiss_coverage_for_files(partial: dict[str, float], files: list[str]) -> dict[str, float]:
    return {path: partial.get(path, 100.0) for path in files}


def _is_test_module_path(path: str) -> bool:
    return path.startswith("tests/") or path.endswith("/tests")


def compare_coverage(true: dict[str, float], kiss_partial: dict[str, float]) -> CoverageComparison:
    common = sorted(
        p
        for p in true
        if not (_is_test_module_path(p) and true[p] > 50.0)
    )
    kiss = kiss_coverage_for_files(kiss_partial, common)
    true_vals = [true[path] for path in common]
    kiss_vals = [kiss[path] for path in common]
    errors = [abs(t - k) for t, k in zip(true_vals, kiss_vals, strict=True)]
    return CoverageComparison(common, true_vals, kiss_vals, errors)


def report_metrics(comparison: CoverageComparison) -> None:
    if not comparison.errors:
        print("No overlapping files between runtime coverage and kiss analysis.")
        return

    scale = 100.0
    errors_01 = [e / scale for e in comparison.errors]
    mean_err = statistics.mean(errors_01)
    std_err = statistics.stdev(errors_01) if len(errors_01) > 1 else 0.0
    mean_plus_std = mean_err + std_err
    corr = spearmanr(comparison.true_vals, comparison.kiss_vals).statistic
    if corr is None:
        corr = float("nan")

    n_files = len(comparison.errors)
    print(f"files compared: {n_files}")
    print(f"mean(c_f): {mean_err:.4f}")
    print(f"mean+std(c_f): {mean_plus_std:.4f}")
    print(f"p50(c_f):  {percentile(errors_01, 50):.4f}")
    print(f"p90(c_f):  {percentile(errors_01, 90):.4f}")
    print(f"p99(c_f):  {percentile(errors_01, 99):.4f}")
    print(f"max(c_f):  {max(errors_01):.4f}")
    print(f"spearman(coverage_true, coverage_kiss): {corr:.4f}")


def run_comparison(repo: Path) -> None:
    """Collect runtime and kiss coverage for REPO and print comparison metrics."""
    true = run_true_coverage(repo)
    kiss_partial = run_kiss_check_all(repo)
    comparison = compare_coverage(true, kiss_partial)
    report_metrics(comparison)
