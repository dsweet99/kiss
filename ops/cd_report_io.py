from __future__ import annotations

import json
from pathlib import Path

import click

from ops.cd_discrepancy_report import DiscrepancyReport
def print_report(r: DiscrepancyReport) -> None:
    click.echo(f"repo: {r.repo}")
    click.echo(f"language: {r.language}")
    click.echo(f"aligned_files: {r.n_files}")
    click.echo(f"kiss_median_static_pct: {r.kiss_median_pct:.1f}")
    click.echo(f"runtime_total_line_pct: {r.runtime_total_pct:.1f}")
    click.echo(f"global_gap: {r.global_gap:.1f}")
    click.echo(f"file_mae: {r.file_mae:.1f}")
    click.echo(f"file_rmse: {r.file_rmse:.3f}")
    click.echo(f"spearman: {r.spearman if r.spearman is not None else 'n/a'}")
    click.echo(f"inflation_rate (kiss >= runtime+20): {r.inflation_rate:.3f}")
    click.echo(f"blind_spot_rate (runtime >= kiss+20): {r.blind_spot_rate:.3f}")
    click.echo(f"discrepancy_score (file_rmse): {r.file_rmse:.3f}")


def _display_path(repo: Path, path: Path) -> str:
    try:
        return str(path.relative_to(repo))
    except ValueError:
        return str(path)


def print_detailed_report(r: DiscrepancyReport) -> None:
    rows = sorted(r.pairs, key=lambda p: p.abs_delta, reverse=True)
    click.echo("")
    click.echo(
        f"file details ({len(rows)} aligned files, sorted by |delta| descending):"
    )
    click.echo(f"{'file':<72} {'kiss':>6} {'runtime':>7} {'delta':>7}  flag")
    for row in rows:
        delta_s = f"{row.delta:+.1f}"
        click.echo(
            f"{_display_path(r.repo, row.path):<72} "
            f"{row.kiss_pct:6.1f} {row.runtime_pct:7.1f} {delta_s:>7}  {row.flag}"
        )


def write_report_json(r: DiscrepancyReport, out_path: Path) -> None:
    rows = sorted(r.pairs, key=lambda p: p.abs_delta, reverse=True)
    payload = {
        "repo": str(r.repo),
        "language": r.language,
        "summary": {
            "aligned_files": r.n_files,
            "kiss_median_static_pct": r.kiss_median_pct,
            "runtime_total_line_pct": r.runtime_total_pct,
            "global_gap": r.global_gap,
            "file_mae": r.file_mae,
            "file_rmse": r.file_rmse,
            "spearman": r.spearman,
            "inflation_rate": r.inflation_rate,
            "blind_spot_rate": r.blind_spot_rate,
        },
        "files": [
            {
                "file": _display_path(r.repo, row.path),
                "kiss_pct": row.kiss_pct,
                "runtime_pct": row.runtime_pct,
                "delta": row.delta,
                "abs_delta": row.abs_delta,
                "flag": row.flag,
            }
            for row in rows
        ],
    }
    out_path.write_text(json.dumps(payload, indent=2) + "\n")


def emit_report(
    r: DiscrepancyReport, *, detailed: bool, report_out: Path | None
) -> None:
    print_report(r)
    if detailed:
        print_detailed_report(r)
    if report_out is not None:
        write_report_json(r, report_out)
        click.echo(f"report written: {report_out.resolve()}")


__all__ = ["print_report", "print_detailed_report", "write_report_json", "emit_report"]
