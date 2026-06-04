"""Parse kiss check --all output into per-file coverage percentages."""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

from python.coverage_stats import normalize_path

KISS_VIOLATION_RE = re.compile(
    r"^VIOLATION:test_coverage:(?P<file>[^:]+):\d+:[^:]+: (?P<pct>\d+)% covered"
)


def run_kiss_check_all(repo: Path) -> dict[str, float]:
    cmd = ["kiss", "check", "--all", str(repo.resolve())]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(
            "kiss check --all failed\n"
            f"command: {' '.join(cmd)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )

    partial: dict[str, float] = {}
    for line in result.stdout.splitlines():
        match = KISS_VIOLATION_RE.match(line)
        if match is None:
            continue
        rel = normalize_path(match.group("file"), repo)
        partial[rel] = float(match.group("pct"))
    return partial
