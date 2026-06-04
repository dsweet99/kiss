#!/usr/bin/env python3
"""CLI entry point: compare kiss static test-reference coverage to runtime line coverage."""

from __future__ import annotations

import sys
from pathlib import Path

import click


def _ensure_import_path() -> None:
    root = str(Path(__file__).resolve().parent.parent)
    if root not in sys.path:
        sys.path.insert(0, root)


@click.command()
@click.argument(
    "repo",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, path_type=Path),
)
def main(repo: Path) -> None:
    """Compare kiss coverage estimates to runtime line coverage for REPO."""
    _ensure_import_path()
    from python.coverage_metrics import run_comparison

    try:
        run_comparison(repo.resolve())
    except RuntimeError as exc:
        raise click.ClickException(str(exc)) from exc


if __name__ == "__main__":
    main()
