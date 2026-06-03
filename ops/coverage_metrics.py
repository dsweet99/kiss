#!/usr/bin/env python3
"""CLI entry point: compare kiss static test-reference coverage to runtime line coverage."""

from __future__ import annotations

from pathlib import Path

import click

from python.coverage_metrics import run_comparison


@click.command()
@click.argument(
    "repo",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, path_type=Path),
)
def main(repo: Path) -> None:
    """Compare kiss coverage estimates to runtime line coverage for REPO."""
    run_comparison(repo.resolve())


if __name__ == "__main__":
    main()
