"""Tests for adversarial multi-repo helpers."""

from __future__ import annotations

from pathlib import Path

import pytest
from python.adversarial_multi_repo import (
    adversarial_prompt_path,
    format_repo_paths,
    normalize_repos,
)


def test_normalize_repos_deduplicates(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    paths = normalize_repos([repo, repo])
    assert paths == (repo.resolve(),)


def test_normalize_repos_requires_at_least_one() -> None:
    with pytest.raises(ValueError, match="at least one"):
        normalize_repos([])


def test_adversarial_prompt_path_single_repo(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    path = adversarial_prompt_path(tmp_path / "kiss", (repo.resolve(),), "fix")
    assert path == Path(f"{repo.resolve()}_fix_prompt.md")


def test_format_repo_paths(tmp_path: Path) -> None:
    a = tmp_path / "a"
    b = tmp_path / "b"
    a.mkdir()
    b.mkdir()
    text = format_repo_paths((a.resolve(), b.resolve()))
    assert str(a.resolve()) in text
    assert str(b.resolve()) in text


def test_adversarial_prompt_path_multiple_repos(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    a = tmp_path / "a"
    b = tmp_path / "b"
    a.mkdir()
    b.mkdir()
    path = adversarial_prompt_path(kiss, (a.resolve(), b.resolve()), "fix")
    assert path == kiss / ".adversarial_fix_a_b_prompt.md"
