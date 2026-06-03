"""Compare kiss static test-reference coverage to runtime line coverage."""

from __future__ import annotations

import re
import statistics
import subprocess
from pathlib import Path
from typing import NamedTuple

import click
from scipy.stats import spearmanr

from python.coverage_collect import run_true_coverage
from python.coverage_stats import normalize_path, percentile

KISS_VIOLATION_RE = re.compile(
    r"^VIOLATION:test_coverage:(?P<file>[^:]+):\d+:[^:]+: (?P<pct>\d+)% covered"
)


class CoverageComparison(NamedTuple):
    paths: list[str]
    true_vals: list[float]
    kiss_vals: list[float]
    errors: list[float]


def run_kiss_check_all(repo: Path) -> dict[str, float]:
    cmd = ["kiss", "check", "--all", str(repo.resolve())]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise click.ClickException(
            "kiss check --all failed\n"
            f"command: {' '.join(cmd)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )

    # ``kiss check --all`` emits ``test_coverage`` violations for unreferenced
    # definitions; the message carries the file-level percentage. Files with
    # no violations are treated as 100% covered (all definitions referenced).
    partial: dict[str, float] = {}
    for line in result.stdout.splitlines():
        match = KISS_VIOLATION_RE.match(line)
        if match is None:
            continue
        rel = normalize_path(match.group("file"), repo)
        partial[rel] = float(match.group("pct"))
    return partial


def kiss_coverage_for_files(partial: dict[str, float], files: list[str]) -> dict[str, float]:
    return {path: partial.get(path, 100.0) for path in files}


def compare_coverage(true: dict[str, float], kiss_partial: dict[str, float]) -> CoverageComparison:
    common = sorted(true)
    kiss = kiss_coverage_for_files(kiss_partial, common)
    true_vals = [true[path] for path in common]
    kiss_vals = [kiss[path] for path in common]
    errors = [abs(t - k) for t, k in zip(true_vals, kiss_vals, strict=True)]
    return CoverageComparison(common, true_vals, kiss_vals, errors)


def report_metrics(comparison: CoverageComparison) -> None:
    if not comparison.errors:
        click.echo("No overlapping files between runtime coverage and kiss analysis.")
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
    click.echo(f"files compared: {n_files}")
    click.echo(f"mean(c_f): {mean_err:.4f}")
    click.echo(f"mean+std(c_f): {mean_plus_std:.4f}")
    click.echo(f"p50(c_f):  {percentile(errors_01, 50):.4f}")
    click.echo(f"p90(c_f):  {percentile(errors_01, 90):.4f}")
    click.echo(f"p99(c_f):  {percentile(errors_01, 99):.4f}")
    click.echo(f"max(c_f):  {max(errors_01):.4f}")
    click.echo(f"spearman(coverage_true, coverage_kiss): {corr:.4f}")


def run_comparison(repo: Path) -> None:
    """Collect runtime and kiss coverage for REPO and print comparison metrics."""
    true = run_true_coverage(repo)
    kiss_partial = run_kiss_check_all(repo)
    comparison = compare_coverage(true, kiss_partial)
    report_metrics(comparison)
