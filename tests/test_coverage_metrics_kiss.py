"""Tests for kiss subprocess integration in python coverage helpers."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
import python.coverage_kiss as coverage_kiss


def test_run_kiss_check_all_parses_violations(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    violation = (
        "VIOLATION:test_coverage:pkg/a.py:10:foo: 42% covered. "
        "Add test coverage for this code unit."
    )

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 1, violation, "")

    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    got = coverage_kiss.run_kiss_check_all(repo)
    assert got == {"pkg/a.py": 42.0}


def test_kiss_binary_prefers_env_override(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("KISS_BIN", "/tmp/custom-kiss")
    assert coverage_kiss._kiss_binary() == "/tmp/custom-kiss"


def test_run_kiss_check_all_uses_resolved_repo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    seen: dict[str, list[str]] = {}
    monkeypatch.setenv("KISS_BIN", "kiss-custom")

    def fake_run(cmd: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        seen["cmd"] = cmd
        assert kwargs["capture_output"] is True
        assert kwargs["text"] is True
        return subprocess.CompletedProcess(cmd, 0, "", "")

    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    assert coverage_kiss.run_kiss_check_all(repo) == {}
    assert seen["cmd"] == ["kiss-custom", "check", "--all", str(repo.resolve())]


def test_run_kiss_check_all_rc_zero_ok(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 0, "", "")

    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    assert coverage_kiss.run_kiss_check_all(repo) == {}


def test_run_kiss_check_all_bad_rc_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 2, "boom", "err")

    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    with pytest.raises(RuntimeError, match="kiss check --all failed"):
        coverage_kiss.run_kiss_check_all(repo)
