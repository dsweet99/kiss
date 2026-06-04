"""Tests for python.coverage_metrics comparison and CLI."""

from __future__ import annotations

import random
from pathlib import Path

import pytest
from click.testing import CliRunner

import python.coverage_metrics as metrics
from ops.coverage_metrics import main as coverage_metrics_main


def test_main_help() -> None:
    runner = CliRunner()
    result = runner.invoke(coverage_metrics_main, ["--help"])
    assert result.exit_code == 0
    assert coverage_metrics_main.__doc__ is not None


def test_coverage_comparison_namedtuple_fields() -> None:
    row = metrics.CoverageComparison(["a.py"], [1.0], [2.0], [1.0])
    assert row.paths == ["a.py"]
    assert row.errors == [1.0]


def test_compare_coverage_skips_high_runtime_test_modules() -> None:
    true = {"tests/test_foil.py": 100.0, "src/a.py": 10.0}
    kiss_partial = {"tests/test_foil.py": 0.0, "src/a.py": 8.0}
    comparison = metrics.compare_coverage(true, kiss_partial)
    assert comparison.paths == ["src/a.py"]
    assert comparison.true_vals == [10.0]
    assert comparison.kiss_vals == [8.0]


def test_compare_coverage_errors_and_ordering() -> None:
    true = {"b.py": 80.0, "a.py": 60.0}
    kiss_partial = {"a.py": 50.0}
    comparison = metrics.compare_coverage(true, kiss_partial)
    assert comparison.paths == ["a.py", "b.py"]
    assert comparison.true_vals == [60.0, 80.0]
    assert comparison.kiss_vals == [50.0, 100.0]
    assert comparison.errors == [10.0, 20.0]


def test_compare_coverage_metamorphic_kiss_only_keys_ignored() -> None:
    seed = 998877
    rng = random.Random(seed)
    print(f"compare_coverage metamorphic seed={seed}")
    true = {f"f{i}.py": rng.uniform(0, 100) for i in range(5)}
    kiss_partial = {f"extra{i}.py": 0.0 for i in range(3)}
    comparison = metrics.compare_coverage(true, kiss_partial)
    assert len(comparison.errors) == len(true)
    for err in comparison.errors:
        assert 0.0 <= err <= 100.0


def test_report_metrics_empty_errors(capsys: pytest.CaptureFixture[str]) -> None:
    metrics.report_metrics(metrics.CoverageComparison([], [], [], []))
    out = capsys.readouterr().out
    assert "No overlapping files" in out


def test_report_metrics_scaled_values(capsys: pytest.CaptureFixture[str]) -> None:
    """Two-file fixture with errors [0, 10] percentage points -> [0, 0.1] normalized."""
    metrics.report_metrics(
        metrics.CoverageComparison(
            ["a.py", "b.py"],
            [100.0, 50.0],
            [100.0, 40.0],
            [0.0, 10.0],
        )
    )
    out = capsys.readouterr().out
    assert "files compared: 2" in out
    assert "mean(c_f): 0.0500" in out
    assert "mean+std(c_f): 0.1207" in out
    assert "p50(c_f):  0.0500" in out
    assert "max(c_f):  0.1000" in out
    assert "spearman(coverage_true, coverage_kiss):" in out


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
