"""Emit JSON kiss/runtime coverage maps for a repo (subprocess entry point)."""

from __future__ import annotations

import json
import sys
from pathlib import Path


def main() -> None:
    repo = Path(sys.argv[1]).resolve()
    from python.coverage_collect import run_true_coverage
    from python.coverage_kiss import run_kiss_check_all

    payload = {
        "kiss": run_kiss_check_all(repo),
        "true": run_true_coverage(repo),
    }
    sys.stdout.write(json.dumps(payload))


if __name__ == "__main__":
    main()
