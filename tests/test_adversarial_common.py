"""Tests for adversarial path helpers."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import python.adversarial_common as common


def test_calibration_repos_root_and_adversarial_root_are_siblings_of_repo_root(
    tmp_path: Path, monkeypatch: object
) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(common, "repo_root", lambda: kiss)
    assert common.calibration_repos_root() == tmp_path / "repos"
    assert common.adversarial_root() == tmp_path / "kiss-adversarial"


def test_allocate_adversarial_id_format_and_collision(
    tmp_path: Path, monkeypatch: object
) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(common, "repo_root", lambda: kiss)

    with patch.object(common, "_adversarial_id_suffix", return_value="20260604_143022_abcd"):
        first = common.allocate_adversarial_id("foil")
    assert first == tmp_path / "kiss-adversarial" / "foil" / "20260604_143022_abcd"
    first.parent.mkdir(parents=True)
    first.mkdir()

    with patch.object(common, "_adversarial_id_suffix", return_value="20260604_143022_abcd"):
        second = common.allocate_adversarial_id("foil")
    assert second != first
    assert second.parent == first.parent


def test_discover_calibration_repos_manifest(
    tmp_path: Path, monkeypatch: object
) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(common, "repo_root", lambda: kiss)

    (tmp_path / "repos" / "a").mkdir(parents=True)
    (tmp_path / "repos" / ".malvin").mkdir(parents=True)
    (tmp_path / "repos" / "loose.txt").write_text("x", encoding="utf-8")
    (tmp_path / "kiss-adversarial" / "foil" / "x").mkdir(parents=True)
    (tmp_path / "kiss-adversarial" / "cheat" / "y").mkdir(parents=True)

    discovered = common.discover_calibration_repos()
    assert discovered == [
        ("repos", tmp_path / "repos" / "a"),
        ("foil", tmp_path / "kiss-adversarial" / "foil" / "x"),
        ("cheat", tmp_path / "kiss-adversarial" / "cheat" / "y"),
    ]


def test_discover_calibration_repos_missing_parent_yields_empty_branch(
    tmp_path: Path, monkeypatch: object
) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(common, "repo_root", lambda: kiss)
    assert common.discover_calibration_repos() == []
