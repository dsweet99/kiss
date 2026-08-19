"""CLI entry point: compare kiss cached line coverage to independent coverage."""

from __future__ import annotations

import sys
from pathlib import Path


def _ensure_import_path() -> None:
    root = str(Path(__file__).resolve().parent.parent)
    if root not in sys.path:
        sys.path.insert(0, root)


def main() -> None:
    _ensure_import_path()
    from python.coverage_metrics import coverage_metrics_cli

    coverage_metrics_cli()


if __name__ == "__main__":
    main()
