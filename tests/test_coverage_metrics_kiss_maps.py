"""Additional tests for kiss subprocess integration in python coverage helpers."""

from __future__ import annotations

import json
import runpy
import sys
from pathlib import Path

import pytest
import python.coverage_kiss as coverage_kiss
import python.coverage_metrics as metrics


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

def test_coverage_maps_cli_module_entrypoint(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setattr(coverage_kiss, "run_kiss_check_all", lambda _r: {"src/a.py": 55.0})
    monkeypatch.setattr(
        "python.coverage_collect.run_true_coverage",
        lambda _r: {"src/a.py": 44.0},
    )
    monkeypatch.setattr(sys, "argv", ["coverage_maps_cli.py", str(repo)])

    runpy.run_module("python.coverage_maps_cli", run_name="__main__")

    payload = json.loads(capsys.readouterr().out)
    assert payload == {"kiss": {"src/a.py": 55.0}, "true": {"src/a.py": 44.0}}

def test_kiss_coverage_for_files_defaults_missing_to_zero() -> None:
    partial = {"a.py": 50.0}
    got = metrics.kiss_coverage_for_files(partial, ["a.py", "b.py"])
    assert got == {"a.py": 50.0, "b.py": 0.0}
