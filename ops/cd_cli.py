from __future__ import annotations

import sys
from pathlib import Path

import click

from ops.cd_analyze import RuntimeCoverage, analyze_discrepancy as analyze
from ops.cd_click import cli, report_options
from ops.cd_python_run import PythonCoverageRun, python_cmd
from ops.cd_report_io import emit_report
from ops.cd_runtime import llvm_cov_per_file


class _PythonCoverageCommand(click.Command):
    def invoke(self, ctx: click.Context) -> int:
        repo = ctx.params["repo"]
        pytest_args = ctx.params["pytest_args"]
        python_cmd(
            PythonCoverageRun(
                repo,
                ctx.params["source"],
                pytest_args if pytest_args else ("tests/",),
                ctx.params["detailed"],
                ctx.params["report_out"],
            )
        )
        return 0


def _python_coverage_params() -> list[click.Parameter]:
    return [
        click.Argument(
            ["repo"],
            type=click.Path(
                exists=True, file_okay=False, resolve_path=True, path_type=Path
            ),
        ),
        click.Option(
            ["--source"],
            default=None,
            help="Passed to slipcover --source (e.g. 'rich' for the rich package tree).",
        ),
        click.Option(
            ["--detailed"],
            is_flag=True,
            default=False,
            help="Print file-by-file kiss vs runtime coverage after the summary.",
        ),
        click.Option(
            ["--report-out"],
            type=click.Path(dir_okay=False, path_type=Path),
            default=None,
            help="Write full summary + per-file details as JSON to PATH.",
        ),
        click.Argument(["pytest_args"], nargs=-1, default=("tests/",)),
    ]


def register_python_command(group: click.Group) -> None:
    group.add_command(
        _PythonCoverageCommand(
            "python",
            params=_python_coverage_params(),
            help="Measure discrepancy for a Python repo (runtime via slipcover + pytest).",
        )
    )


register_python_command(cli)


@cli.command("rust")
@click.argument(
    "repo",
    type=click.Path(exists=True, file_okay=False, resolve_path=True, path_type=Path),
)
@report_options
def rust_cmd(repo: Path, detailed: bool, report_out: Path | None) -> None:
    """Measure discrepancy for a Rust repo (runtime via cargo-llvm-cov)."""
    runtime_map, runtime_total = llvm_cov_per_file(repo)
    emit_report(
        analyze(repo, "rust", RuntimeCoverage(runtime_map, runtime_total)),
        detailed=detailed,
        report_out=report_out,
    )


def main() -> None:
    try:
        cli(prog_name="coverage_discrepancy")
    except RuntimeError as exc:
        click.echo(f"error: {exc}", err=True)
        sys.exit(1)
