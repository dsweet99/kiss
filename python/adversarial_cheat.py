"""Adversarial cheat: repos that pass kiss static coverage but not runtime tools."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple

from python.adversarial_common import repo_root

TRUE_COVERAGE_CEILING = 80.0


class CheatMetrics(NamedTuple):
    kiss_passes: bool
    gaps: tuple[tuple[str, float, float], ...]


def _is_test_module_path(path: str) -> bool:
    return path.startswith("tests/") or path.endswith("/tests")


def cheat_gaps(kiss_partial: dict[str, float], true: dict[str, float]) -> list[tuple[str, float, float]]:
    gaps: list[tuple[str, float, float]] = []
    for path, true_pct in sorted(true.items()):
        if _is_test_module_path(path):
            continue
        kiss_pct = kiss_partial.get(path, 100.0)
        if kiss_pct >= 100.0 and true_pct < TRUE_COVERAGE_CEILING:
            gaps.append((path, kiss_pct, true_pct))
    return gaps


def cheat_satisfied(metrics: CheatMetrics) -> bool:
    return metrics.kiss_passes and len(metrics.gaps) > 0


def format_cheat_report(metrics: CheatMetrics) -> str:
    lines = [f"kiss check: {'pass' if metrics.kiss_passes else 'fail'}"]
    if not metrics.gaps:
        lines.append("source files with kiss=100% but low runtime coverage: (none)")
    else:
        lines.append("source files with kiss=100% but low runtime coverage:")
        for path, kiss_pct, true_pct in metrics.gaps:
            lines.append(f"  {path}: kiss={kiss_pct:.0f}% true={true_pct:.0f}%")
    lines.append(f"cheat gap count: {len(metrics.gaps)}")
    return "\n".join(lines)


def build_cheat_prompt(kiss_root: Path, repo_dir: Path, lang: str) -> str:
    adversarial_py = (kiss_root / "ops" / "adversarial.py").resolve()
    lang_instruction = {
        "rust": (
            "Rust only (include `Cargo.toml`, tests runnable via "
            "`cargo llvm-cov nextest`)."
        ),
        "python": "Python only (include tests runnable via `pytest` with slipcover).",
        "both": "Both Rust and Python in the same repo.",
    }[lang]
    return f"""# Cheat kiss: static coverage without runtime execution

Create a self-contained repository at:

  {repo_dir.resolve()}

Use this directory exactly; do not relocate the repo.

## Language

{lang_instruction}

## Goal

Write unit tests that **satisfy kiss** (all definitions statically referenced, so
`kiss check --all` exits 0 with no `test_coverage` gate failures) but **do not**
satisfy runtime line coverage (`slipcover` for Python, `cargo llvm-cov` for Rust):
source files should report low line coverage even though kiss treats them as fully
covered.

Favor techniques where tests mention or bind symbols without executing their bodies
(for example bind-only imports, mocks that prevent execution, unreachable branches
still referenced in tests, or string-based lookups that never call the target).

## Verification loop

From the kiss repo root ({kiss_root.resolve()}), repeatedly run:

  {sys.executable} {adversarial_py} cheat-verify {repo_dir.resolve()}

Revise the generated repo until **both** hold:

- `kiss check --all` passes (exit code 0, no test_coverage violations)
- at least one non-test source file has kiss coverage 100% but runtime line coverage
  below {TRUE_COVERAGE_CEILING:.0f}%

Also, make your repo test something different from the other repos (if there are any): {repo_dir.resolve()}/../*

Tests must pass and coverage tools must succeed. Stop when the cheat conditions are
met. Print the final cheat-verify output when done.
"""


def run_kiss_check(repo: Path) -> tuple[int, str]:
    cmd = ["kiss", "check", "--all", str(repo.resolve())]
    result = subprocess.run(cmd, capture_output=True, text=True, check=False)
    combined = result.stdout
    if result.stderr:
        combined = f"{combined}\n{result.stderr}" if combined else result.stderr
    return result.returncode, combined


def _coverage_maps_subprocess_env() -> dict[str, str]:
    root = str(repo_root())
    existing = os.environ.get("PYTHONPATH", "")
    pythonpath = root if not existing else f"{root}{os.pathsep}{existing}"
    return {**os.environ, "PYTHONPATH": pythonpath}


def _load_coverage_maps(repo: Path) -> tuple[dict[str, float], dict[str, float]]:
    import json

    script = Path(__file__).resolve().parent / "coverage_maps_cli.py"
    cmd = [sys.executable, str(script), str(repo.resolve())]
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        check=False,
        env=_coverage_maps_subprocess_env(),
    )
    if result.returncode != 0:
        msg = result.stderr or result.stdout or "coverage maps subprocess failed"
        raise RuntimeError(msg)
    payload = json.loads(result.stdout)
    kiss_partial = {str(k): float(v) for k, v in payload["kiss"].items()}
    true = {str(k): float(v) for k, v in payload["true"].items()}
    return kiss_partial, true


def verify_cheat(kiss_root: Path, repo: Path) -> tuple[bool, CheatMetrics, str]:
    del kiss_root
    exit_code, kiss_output = run_kiss_check(repo)
    kiss_passes = exit_code == 0
    kiss_partial, true = _load_coverage_maps(repo)
    gaps = cheat_gaps(kiss_partial, true)
    metrics = CheatMetrics(kiss_passes, tuple(gaps))
    report = format_cheat_report(metrics)
    combined = kiss_output
    if combined:
        combined = f"{combined.rstrip()}\n\n{report}"
    else:
        combined = report
    return cheat_satisfied(metrics), metrics, combined
