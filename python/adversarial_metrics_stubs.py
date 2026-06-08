"""Shared stubs for adversarial metrics CLI tests."""

from __future__ import annotations

from pathlib import Path

import pytest

import python.adversarial_common as common


def stub_metrics_manifest(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(common, "repo_root", lambda: kiss)
    (tmp_path / "repos" / "real").mkdir(parents=True)
    (tmp_path / "kiss-adversarial" / "foil" / "a").mkdir(parents=True)
    (tmp_path / "kiss-adversarial" / "cheat" / "b").mkdir(parents=True)


def print_good_comparison_output() -> None:
    print("files compared: 3")
    print("mean+std(c_f): 0.1200")
    print("spearman(coverage_true, coverage_kiss): 0.8500")
