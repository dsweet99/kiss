
from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path


def normalize_repos(repos: Sequence[Path]) -> tuple[Path, ...]:
    if not repos:
        msg = "at least one repository path is required"
        raise ValueError(msg)
    resolved = tuple(Path(p).resolve() for p in repos)
    seen: set[Path] = set()
    unique: list[Path] = []
    for path in resolved:
        if path not in seen:
            seen.add(path)
            unique.append(path)
    return tuple(unique)


def adversarial_prompt_path(
    kiss_root: Path, repos: tuple[Path, ...], suffix: str
) -> Path:
    if len(repos) == 1:
        return Path(f"{repos[0]}_{suffix}_prompt.md")
    label = "_".join(r.name for r in repos)
    if len(label) > 80:
        label = f"{len(repos)}_repos"
    return kiss_root / f".adversarial_{suffix}_{label}_prompt.md"


def format_repo_paths(repos: tuple[Path, ...]) -> str:
    return "\n".join(f"  {p}" for p in repos)
