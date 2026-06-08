"""Tests for kiss binary resolution in python coverage helpers."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

import python.coverage_kiss as coverage_kiss


def test_kiss_binary_prefers_release_build(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    release_dir = tmp_path / "target" / "release"
    release_dir.mkdir(parents=True)
    release_bin = release_dir / "kiss"
    release_bin.write_text("", encoding="utf-8")
    monkeypatch.setattr(coverage_kiss, "KISS_REPO_ROOT", tmp_path)
    monkeypatch.delenv("KISS_BIN", raising=False)
    assert coverage_kiss._kiss_binary() == str(release_bin)


def test_kiss_binary_honors_kiss_bin_override(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("KISS_BIN", "/custom/kiss")
    assert coverage_kiss._kiss_binary() == "/custom/kiss"


def test_run_kiss_check_all_uses_resolved_binary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    seen: list[list[str]] = []

    def fake_run(cmd, **kwargs):
        seen.append(cmd)
        return subprocess.CompletedProcess(cmd, 0, "", "")

    monkeypatch.setattr(coverage_kiss, "_kiss_binary", lambda: "/resolved/kiss")
    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    coverage_kiss.run_kiss_check_all(repo)
    assert seen == [["/resolved/kiss", "check", "--all", str(repo.resolve())]]
