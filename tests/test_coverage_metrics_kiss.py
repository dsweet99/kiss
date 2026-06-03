"""Tests for kiss subprocess integration in python.coverage_metrics."""

from __future__ import annotations

import subprocess
from pathlib import Path

import click
import pytest

import python.coverage_metrics as metrics


def test_run_kiss_check_all_parses_violations(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    violation = (
        "VIOLATION:test_coverage:pkg/a.py:10:foo: 42% covered. "
        "Add test coverage for this code unit."
    )

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 1, violation, "")

    monkeypatch.setattr(metrics.subprocess, "run", fake_run)
    got = metrics.run_kiss_check_all(repo)
    assert got == {"pkg/a.py": 42.0}


def test_run_kiss_check_all_rc_zero_ok(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 0, "", "")

    monkeypatch.setattr(metrics.subprocess, "run", fake_run)
    assert metrics.run_kiss_check_all(repo) == {}


def test_run_kiss_check_all_bad_rc_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 2, "boom", "err")

    monkeypatch.setattr(metrics.subprocess, "run", fake_run)
    with pytest.raises(click.ClickException, match="kiss check --all failed"):
        metrics.run_kiss_check_all(repo)


def test_kiss_coverage_for_files_defaults_missing_to_100() -> None:
    partial = {"a.py": 50.0}
    got = metrics.kiss_coverage_for_files(partial, ["a.py", "b.py"])
    assert got == {"a.py": 50.0, "b.py": 100.0}
