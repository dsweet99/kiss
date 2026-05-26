#!/usr/bin/env python3
"""Compare kiss static test-name coverage vs runtime line coverage."""

from __future__ import annotations

import json
import math
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import click

KISS_ROOT = Path(__file__).resolve().parents[1]


DISCREPANCY_THRESHOLD = 20
MAX_COVERAGE_PCT = 100.0


@dataclass(frozen=True)
class FileCoverage:
    path: Path
    kiss_pct: float
    runtime_pct: float

    @property
    def delta(self) -> float:
        return self.kiss_pct - self.runtime_pct

    @property
    def abs_delta(self) -> float:
        return abs(self.delta)

    @property
    def flag(self) -> str:
        if self.delta >= DISCREPANCY_THRESHOLD:
            return "inflated"
        if self.delta <= -DISCREPANCY_THRESHOLD:
            return "blind_spot"
        return ""


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


def run(cmd: list[str], *, cwd: Path | None = None, check: bool = True) -> str:
    proc = subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout


def kiss_per_file(repo: Path) -> dict[Path, float]:
    repo = repo.resolve()
    binary = KISS_ROOT / "target" / "debug" / "kiss-coverage-map"
    for cmd, cwd in (
        ([str(binary), "."], repo),
        (
            [
                "cargo",
                "run",
                "-q",
                "--manifest-path",
                str(KISS_ROOT / "Cargo.toml"),
                "--bin",
                "kiss-coverage-map",
                "--",
                ".",
            ],
            repo,
        ),
    ):
        try:
            proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)
        except FileNotFoundError:
            continue
        if proc.returncode == 0 and proc.stdout.strip():
            raw = json.loads(proc.stdout)
            return {Path(k).resolve(): float(v) for k, v in raw.items()}
    raise RuntimeError(f"could not obtain kiss per-file coverage for {repo}")


def kiss_summary_median(repo: Path) -> float:
    out = run(["kiss", "stats", str(repo)], cwd=repo)
    inv_p50 = 0
    for line in out.splitlines():
        if line.startswith("inv_test_coverage"):
            parts = line.split()
            if len(parts) >= 3:
                inv_p50 = int(parts[2])
    return 100.0 - inv_p50


def llvm_cov_per_file(repo: Path) -> tuple[dict[Path, float], float]:
    for cmd in (
        ["cargo", "llvm-cov", "nextest", "--lib", "--summary-only"],
        ["cargo", "llvm-cov", "--lib", "--summary-only"],
        ["cargo", "llvm-cov", "--summary-only"],
    ):
        proc = subprocess.run(cmd, cwd=repo, text=True, capture_output=True)
        if proc.returncode == 0:
            break
    else:
        raise RuntimeError(f"cargo llvm-cov failed in {repo}")

    report = run(["cargo", "llvm-cov", "report", "--json", "--summary-only"], cwd=repo)
    data = json.loads(report)
    per_file: dict[Path, float] = {}
    total_lines = covered_lines = 0
    for entry in data.get("data", []):
        for f in entry.get("files", []):
            path = Path(f["filename"]).resolve()
            lines = f.get("summary", {}).get("lines", {})
            count = int(lines.get("count", 0))
            covered = int(lines.get("covered", 0))
            pct = float(lines.get("percent", 0.0))
            if count > 0:
                per_file[path] = pct
                total_lines += count
                covered_lines += covered
    total_pct = (100.0 * covered_lines / total_lines) if total_lines else 0.0
    return per_file, total_pct


def slipcover_per_file(
    repo: Path, pytest_args: list[str], *, source: str | None = None
) -> tuple[dict[Path, float], float]:
    import tempfile

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        out_path = tmp.name
    cmd = ["slipcover", "--json", "--out", out_path]
    if source:
        cmd.extend(["--source", source])
    cmd.extend(["-m", "pytest", *pytest_args])
    proc = subprocess.run(cmd, cwd=repo, text=True, capture_output=True)
    if not Path(out_path).exists() or Path(out_path).stat().st_size == 0:
        raise RuntimeError(
            f"slipcover failed ({proc.returncode}): {proc.stderr or proc.stdout}"
        )
    data = json.loads(Path(out_path).read_text())
    per_file: dict[Path, float] = {}
    total_lines = covered_lines = 0
    for rel, info in data.get("files", {}).items():
        path = (repo / rel).resolve()
        summ = info.get("summary", {})
        n_cov = int(summ.get("covered_lines", 0))
        n_miss = int(summ.get("missing_lines", 0))
        total = n_cov + n_miss
        pct = 100.0 * n_cov / total if total else 0.0
        if total > 0:
            per_file[path] = pct
            total_lines += total
            covered_lines += n_cov
    summary = data.get("summary", {})
    total_pct = float(summary.get("percent_covered", 0.0))
    if not total_pct and total_lines:
        total_pct = 100.0 * covered_lines / total_lines
    return per_file, total_pct


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


def analyze(
    repo: Path, language: str, runtime_map: dict[Path, float], runtime_total: float
) -> DiscrepancyReport:
    kiss_map = kiss_per_file(repo)
    pairs = align_files(kiss_map, runtime_map)
    if not pairs:
        raise RuntimeError(f"no overlapping files between kiss and runtime in {repo}")

    diffs = [p.kiss_pct - p.runtime_pct for p in pairs]
    abs_diffs = [abs(d) for d in diffs]
    sq_diffs = [d * d for d in diffs]
    n = len(pairs)
    kiss_median = kiss_summary_median(repo)
    global_gap = abs(kiss_median - runtime_total)
    inflation = sum(1 for d in diffs if d >= 20) / n
    blind = sum(1 for d in diffs if d <= -20) / n
    sp = spearman([p.kiss_pct for p in pairs], [p.runtime_pct for p in pairs])

    file_mae = sum(abs_diffs) / n
    file_rmse = math.sqrt(sum(sq_diffs) / n) / MAX_COVERAGE_PCT

    return DiscrepancyReport(
        repo=repo.resolve(),
        language=language,
        n_files=n,
        kiss_median_pct=kiss_median,
        runtime_total_pct=runtime_total,
        global_gap=global_gap,
        file_mae=file_mae,
        file_rmse=file_rmse,
        spearman=sp,
        inflation_rate=inflation,
        blind_spot_rate=blind,
        pairs=tuple(pairs),
    )


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


@click.group(context_settings={"help_option_names": ["-h", "--help"]})
def cli() -> None:
    """Compare kiss static test-name coverage vs runtime line coverage.

    Requires kiss on PATH, cargo-llvm-cov (rust) or slipcover (python).
    """


@cli.command("rust")
@click.argument(
    "repo",
    type=click.Path(exists=True, file_okay=False, resolve_path=True, path_type=Path),
)
@click.option(
    "--detailed",
    is_flag=True,
    help="Print file-by-file kiss vs runtime coverage after the summary.",
)
@click.option(
    "--report-out",
    type=click.Path(dir_okay=False, path_type=Path),
    default=None,
    help="Write full summary + per-file details as JSON to PATH.",
)
def rust_cmd(repo: Path, detailed: bool, report_out: Path | None) -> None:
    """Measure discrepancy for a Rust repo (runtime via cargo-llvm-cov)."""
    runtime_map, runtime_total = llvm_cov_per_file(repo)
    emit_report(
        analyze(repo, "rust", runtime_map, runtime_total),
        detailed=detailed,
        report_out=report_out,
    )


@cli.command("python")
@click.argument(
    "repo",
    type=click.Path(exists=True, file_okay=False, resolve_path=True, path_type=Path),
)
@click.option(
    "--source",
    "slipcover_source",
    default=None,
    help="Passed to slipcover --source (e.g. 'rich' for the rich package tree).",
)
@click.option(
    "--detailed",
    is_flag=True,
    help="Print file-by-file kiss vs runtime coverage after the summary.",
)
@click.option(
    "--report-out",
    type=click.Path(dir_okay=False, path_type=Path),
    default=None,
    help="Write full summary + per-file details as JSON to PATH.",
)
@click.argument("pytest_args", nargs=-1, default=("tests/",))
def python_cmd(
    repo: Path,
    slipcover_source: str | None,
    detailed: bool,
    report_out: Path | None,
    pytest_args: tuple[str, ...],
) -> None:
    """Measure discrepancy for a Python repo (runtime via slipcover + pytest)."""
    runtime_map, runtime_total = slipcover_per_file(
        repo, list(pytest_args), source=slipcover_source
    )
    emit_report(
        analyze(repo, "python", runtime_map, runtime_total),
        detailed=detailed,
        report_out=report_out,
    )


def main() -> None:
    try:
        cli(prog_name="coverage_discrepancy")
    except RuntimeError as exc:
        click.echo(f"error: {exc}", err=True)
        sys.exit(1)


if __name__ == "__main__":
    main()
