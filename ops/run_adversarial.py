#!/usr/bin/env python3
"""Run the adversarial CLI with all subcommands registered."""

from __future__ import annotations

import sys
from pathlib import Path


def _ensure_import_path() -> None:
    root = str(Path(__file__).resolve().parent.parent)
    if root not in sys.path:
        sys.path.insert(0, root)


def _run() -> None:
    _ensure_import_path()
    from ops.adversarial import main

    main()


if __name__ == "__main__":
    _run()
