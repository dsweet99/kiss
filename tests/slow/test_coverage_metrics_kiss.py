
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
import python.coverage_kiss as coverage_kiss
import python.coverage_metrics as metrics


def test_run_kiss_check_all_parses_violations(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    violation = (
        "VIOLATION:test_coverage:pkg/a.py:10:<file>: 42% covered. "
        "Add test coverage for this file."
    )

    def fake_run(cmd, **kwargs):
        return subprocess.CompletedProcess(cmd, 1, violation, "")

    monkeypatch.setattr(coverage_kiss.subprocess, "run", fake_run)
    got = coverage_kiss.run_kiss_check_all(repo)
    assert got == {"pkg/a.py": 42.0}


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
    with pytest.raises(RuntimeError, match="kiss test --coverage-all failed"):
        coverage_kiss.run_kiss_check_all(repo)


def test_coverage_maps_cli_emits_json(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setattr(coverage_kiss, "run_kiss_check_all", lambda _r: {"a.py": 100.0})
    monkeypatch.setattr(
        "python.coverage_collect.run_true_coverage",
        lambda _r: {"a.py": 10.0},
    )
    from python.coverage_maps_cli import main

    monkeypatch.setattr(sys, "argv", ["coverage_maps_cli", str(repo)])
    main()
    payload = json.loads(capsys.readouterr().out)
    assert payload == {"kiss": {"a.py": 100.0}, "true": {"a.py": 10.0}}


def test_kiss_coverage_for_files_defaults_missing_to_zero() -> None:
    partial = {"a.py": 50.0}
    got = metrics.kiss_coverage_for_files(partial, ["a.py", "b.py"])
    assert got == {"a.py": 50.0, "b.py": 0.0}
