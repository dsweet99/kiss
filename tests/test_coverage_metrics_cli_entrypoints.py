"""Additional tests for python.coverage_metrics comparison and CLI."""

from __future__ import annotations

import runpy
import sys
from pathlib import Path

import pytest
import python.coverage_metrics as metrics
from click.testing import CliRunner
from python.coverage_metrics import coverage_metrics_cli
from python.coverage_metrics import coverage_metrics_cli as coverage_metrics_main


def test_run_comparison_end_to_end(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "mod.py").write_text("x=1\n", encoding="utf-8")

    monkeypatch.setattr(metrics, "run_true_coverage", lambda _repo: {"mod.py": 70.0})
    monkeypatch.setattr(metrics, "run_kiss_check_all", lambda _repo: {"mod.py": 60.0})

    metrics.run_comparison(repo)
    out = capsys.readouterr().out
    assert "files compared: 1" in out
    assert "mean(c_f): 0.1000" in out
    assert "mean+std(c_f): 0.1000" in out

def test_coverage_metrics_cli_raises_click_exception(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import click

    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setattr(
        metrics,
        "run_comparison",
        lambda _repo: (_ for _ in ()).throw(RuntimeError("boom")),
    )
    with pytest.raises(click.ClickException, match="boom"):
        coverage_metrics_cli.callback(repo)

def test_main_end_to_end(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "mod.py").write_text("x=1\n", encoding="utf-8")

    monkeypatch.setattr(metrics, "run_true_coverage", lambda _repo: {"mod.py": 70.0})
    monkeypatch.setattr(metrics, "run_kiss_check_all", lambda _repo: {"mod.py": 60.0})

    runner = CliRunner()
    result = runner.invoke(coverage_metrics_main, [str(repo)])
    assert result.exit_code == 0
    assert "files compared: 1" in result.output
    assert "mean(c_f): 0.1000" in result.output
    assert "mean+std(c_f): 0.1000" in result.output

def test_coverage_metrics_cli_shim_main(monkeypatch: pytest.MonkeyPatch) -> None:
    called: list[str] = []

    def fake_main() -> None:
        called.append("main")

    monkeypatch.setattr(metrics, "coverage_metrics_cli", fake_main)
    monkeypatch.setattr(sys, "argv", ["coverage_metrics_cli.py"])
    runpy.run_module("python.coverage_metrics_cli", run_name="__main__")
    assert called == ["main"]
