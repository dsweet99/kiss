
from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path

from python.adversarial import ParsedMetrics, verify_fix
from python.adversarial_fix_cheat import FixCheatMetrics, verify_fix_cheat
from python.adversarial_multi_repo import normalize_repos


def verify_fix_repos(
    kiss_root: Path, repos: Sequence[Path]
) -> tuple[bool, list[tuple[Path, ParsedMetrics, str]], str]:
    paths = normalize_repos(repos)
    results: list[tuple[Path, ParsedMetrics, str]] = []
    all_passed = True
    sections: list[str] = []
    for repo in paths:
        passed, metrics, output = verify_fix(kiss_root, repo)
        results.append((repo, metrics, output))
        if not passed:
            all_passed = False
        sections.append(f"=== {repo} ===\n{output.rstrip()}")
    return all_passed, results, "\n\n".join(sections)


def verify_fix_cheat_repos(
    kiss_root: Path, repos: Sequence[Path]
) -> tuple[bool, list[tuple[Path, FixCheatMetrics, str]], str]:
    paths = normalize_repos(repos)
    results: list[tuple[Path, FixCheatMetrics, str]] = []
    all_passed = True
    sections: list[str] = []
    for repo in paths:
        passed, metrics, output = verify_fix_cheat(kiss_root, repo)
        results.append((repo, metrics, output))
        if not passed:
            all_passed = False
        sections.append(f"=== {repo} ===\n{output.rstrip()}")
    return all_passed, results, "\n\n".join(sections)
