
from __future__ import annotations

import io
import sys
from dataclasses import dataclass
from pathlib import Path

import click

from python.adversarial_common import CalibrationKind


@dataclass(frozen=True)
class RepoMetricsRow:
    repo: str
    kind: CalibrationKind
    files: str
    mean_std: str
    spearman: str


def parse_files_compared(text: str) -> str:
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("files compared:"):
            return stripped.split(":", 1)[1].strip()
    if "No overlapping files" in text:
        return "0"
    return "n/a"


def format_metric(value: float | None) -> str:
    if value is None:
        return "n/a"
    return f"{value:.4f}"


def measure_repo(
    repo_path: Path,
    kind: CalibrationKind,
    run_comparison: object,
) -> tuple[RepoMetricsRow, str, bool]:
    from python.adversarial import parse_coverage_metrics_output

    buffer = io.StringIO()
    stdout = sys.stdout
    try:
        sys.stdout = buffer
        run_comparison(repo_path)
    except RuntimeError as exc:
        sys.stdout = stdout
        return (
            RepoMetricsRow(repo_path.name, kind, "n/a", "n/a", "n/a"),
            str(exc),
            False,
        )
    finally:
        sys.stdout = stdout

    output = buffer.getvalue()
    parsed = parse_coverage_metrics_output(output)
    row = RepoMetricsRow(
        repo=repo_path.name,
        kind=kind,
        files=parse_files_compared(output),
        mean_std=format_metric(parsed.mean_plus_std),
        spearman=format_metric(parsed.spearman),
    )
    return row, output, True


def print_summary_table(rows: list[RepoMetricsRow]) -> None:
    headers = ("repo", "kind", "files", "mean+std", "spearman")
    widths = [len(h) for h in headers]
    for row in rows:
        values = (row.repo, row.kind, row.files, row.mean_std, row.spearman)
        widths = [max(w, len(v)) for w, v in zip(widths, values, strict=True)]

    def fmt(values: tuple[str, ...]) -> str:
        return "  ".join(v.ljust(w) for v, w in zip(values, widths, strict=True))

    click.echo(fmt(headers))
    click.echo(fmt(tuple("-" * w for w in widths)))
    for row in rows:
        click.echo(
            fmt((row.repo, row.kind, row.files, row.mean_std, row.spearman))
        )


def run_metrics_batch() -> int:
    from python.adversarial_common import discover_calibration_repos
    from python.coverage_metrics import run_comparison

    manifest = discover_calibration_repos()
    rows: list[RepoMetricsRow] = []
    any_failed = False

    for kind, repo_path in manifest:
        click.echo(f"=== {repo_path} ({kind}) ===")
        row, output, ok = measure_repo(repo_path, kind, run_comparison)
        if ok:
            click.echo(output.rstrip())
        else:
            click.echo(f"error: {output}", err=True)
            any_failed = True
        rows.append(row)
        click.echo()

    click.echo("=== summary ===")
    print_summary_table(rows)
    return 1 if any_failed else 0
