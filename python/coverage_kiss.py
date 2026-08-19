
from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path

from python.coverage_stats import normalize_path

KISS_REPO_ROOT = Path(__file__).resolve().parent.parent


def _kiss_binary() -> str:
    override = os.environ.get("KISS_BIN")
    if override:
        return override
    release = KISS_REPO_ROOT / "target" / "release" / "kiss"
    if release.is_file():
        return str(release)
    return "kiss"


KISS_VIOLATION_RE = re.compile(
    r"^VIOLATION:test_coverage:(?P<file>[^:]+):\d+:[^:]+: (?P<pct>\d+)% covered"
)


def _parse_violation_lines(stdout: str, repo: Path) -> dict[str, float]:
    partial: dict[str, float] = {}
    for line in stdout.splitlines():
        match = KISS_VIOLATION_RE.match(line)
        if match is None:
            continue
        rel = normalize_path(match.group("file"), repo)
        partial[rel] = float(match.group("pct"))
    return partial


def run_kiss_check_all(repo: Path) -> dict[str, float]:
    cmd = [_kiss_binary(), "__coverage", "--all", str(repo.resolve())]
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(
            "kiss __coverage --all failed\n"
            f"command: {' '.join(cmd)}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )

    return _parse_violation_lines(result.stdout, repo)
