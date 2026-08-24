
from __future__ import annotations

import sys
from collections.abc import Sequence
from pathlib import Path
from typing import NamedTuple

from python.adversarial_cheat import (
    TRUE_COVERAGE_CEILING,
    CheatMetrics,
    _is_harness_test_module_path,
    _load_coverage_maps,
    cheat_gaps,
    format_cheat_report,
    run_kiss_check,
)
from python.adversarial_multi_repo import format_repo_paths, normalize_repos


class FixCheatMetrics(NamedTuple):
    gaps: tuple[tuple[str, float, float], ...]
    flagged_tests: tuple[str, ...]
    kiss_passes: bool


def cheat_test_paths_flagged(kiss_partial: dict[str, float]) -> list[str]:
    return sorted(p for p in kiss_partial if _is_harness_test_module_path(p))


def fix_cheat_satisfied(metrics: FixCheatMetrics) -> bool:
    return len(metrics.gaps) == 0 and len(metrics.flagged_tests) == 0


def format_fix_cheat_report(metrics: FixCheatMetrics) -> str:
    base = format_cheat_report(CheatMetrics(metrics.kiss_passes, metrics.gaps))
    lines = [base]
    if not metrics.flagged_tests:
        lines.append("test modules flagged by kiss: (none)")
    else:
        lines.append("test modules flagged by kiss:")
        for path in metrics.flagged_tests:
            lines.append(f"  {path}")
    return "\n".join(lines)


def build_fix_cheat_prompt(kiss_root: Path, repos: Sequence[Path]) -> str:
    adversarial_py = (kiss_root / "python" / "adversarial_cli.py").resolve()
    paths = normalize_repos(repos)
    repo_block = format_repo_paths(paths)
    one = len(paths) == 1
    counter_label = (
        "cheat counterexample repository"
        if one
        else "cheat counterexample repositories"
    )
    use_label = "that repo" if one else "those repos"
    verify_cmds = "\n".join(
        f"  {sys.executable} {adversarial_py} fix-cheat-verify {p}"
        for p in paths
    )
    pass_scope = "the repo" if one else "every repo"
    return f"""# Fix kiss: resist static-reference coverage cheating

Edit the kiss codebase at:

  {kiss_root.resolve()}

Do **not** modify the {counter_label} at:

{repo_block}

Use {use_label} only for measurement.

## Goal

Improve kiss's **runtime coverage estimation** so bind-only / non-executing tests no longer
mark **source** code as fully covered. Kiss must flag genuinely untested source even when
tests only reference symbols without executing bodies.

Kiss must **not** apply coverage alignment pressure to **test** modules. Test files may use
static-reference tricks; kiss's job is to estimate runtime line coverage for production code,
not to gate or score test files.

## Measurement loop

From the kiss repo root, repeatedly run fix-cheat-verify on each repo:

{verify_cmds}

Revise kiss until fix-cheat-verify reports on {pass_scope}:

- zero source files with kiss coverage 100% but runtime line coverage below
  {TRUE_COVERAGE_CEILING:.0f}%
- no test-module paths listed under `test modules flagged by kiss`

Do not change the counterexample repos. Their tests must keep passing and coverage tools must
succeed. Stop when both conditions hold on {pass_scope}. Print the final fix-cheat-verify
output for each repo when done.
"""


def verify_fix_cheat(kiss_root: Path, repo: Path) -> tuple[bool, FixCheatMetrics, str]:
    del kiss_root
    exit_code, kiss_output = run_kiss_check(repo)
    kiss_passes = exit_code == 0
    kiss_partial, true = _load_coverage_maps(repo)
    gaps = tuple(cheat_gaps(kiss_partial, true))
    flagged = tuple(cheat_test_paths_flagged(kiss_partial))
    metrics = FixCheatMetrics(gaps, flagged, kiss_passes)
    report = format_fix_cheat_report(metrics)
    combined = kiss_output
    if combined:
        combined = f"{combined.rstrip()}\n\n{report}"
    else:
        combined = report
    return fix_cheat_satisfied(metrics), metrics, combined
