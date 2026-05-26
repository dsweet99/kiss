from __future__ import annotations

from pathlib import Path

import click


@click.group(context_settings={"help_option_names": ["-h", "--help"]})
def cli() -> None:
    """Compare kiss static test-name coverage vs runtime line coverage.

    Requires kiss on PATH, cargo-llvm-cov (rust) or slipcover (python).
    """


def report_options(f):
    f = click.option(
        "--detailed",
        is_flag=True,
        help="Print file-by-file kiss vs runtime coverage after the summary.",
    )(f)
    return click.option(
        "--report-out",
        type=click.Path(dir_okay=False, path_type=Path),
        default=None,
        help="Write full summary + per-file details as JSON to PATH.",
    )(f)

