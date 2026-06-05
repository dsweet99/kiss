#!/usr/bin/env python3
"""CLI entry point: compare kiss static test-reference coverage to runtime line coverage."""

from __future__ import annotations

import sys
from pathlib import Path

_ROOT = str(Path(__file__).resolve().parent.parent)
if _ROOT not in sys.path:
    sys.path.insert(0, _ROOT)

from python.coverage_metrics import coverage_metrics_cli as main  # noqa: E402

if __name__ == "__main__":
    main()
