"""Tests for adversarial metrics batch helpers."""

from __future__ import annotations

from pathlib import Path

import pytest

from python.adversarial_metrics_batch import (
    RepoMetricsRow,
    format_metric,
    measure_repo,
    parse_files_compared,
    run_metrics_batch,
)


def test_parse_files_compared_variants() -> None:
    assert parse_files_compared("files compared: 7\n") == "7"
    assert (
        parse_files_compared("No overlapping files between runtime coverage and kiss analysis.")
        == "0"
    )
    assert parse_files_compared("other output\n") == "n/a"


def test_format_metric() -> None:
    assert format_metric(None) == "n/a"
    assert format_metric(0.123456) == "0.1235"


def test_measure_repo_runtime_error(tmp_path: Path) -> None:
    repo = tmp_path / "sample"
    repo.mkdir()

    def boom(_path: Path) -> None:
        raise RuntimeError("broken")

    row, message, ok = measure_repo(repo, "repos", boom)
    assert ok is False
    assert message == "broken"
    assert row == RepoMetricsRow("sample", "repos", "n/a", "n/a", "n/a")


def test_measure_repo_success(tmp_path: Path) -> None:
    repo = tmp_path / "sample"
    repo.mkdir()

    def fake_run(_path: Path) -> None:
        print("files compared: 2")
        print("mean+std(c_f): 0.1500")
        print("spearman(coverage_true, coverage_kiss): 0.8500")

    row, output, ok = measure_repo(repo, "foil", fake_run)
    assert ok is True
    assert "files compared: 2" in output
    assert row.files == "2"
    assert row.mean_std == "0.1500"
    assert row.spearman == "0.8500"


def test_print_summary_table() -> None:
    import click
    from click.testing import CliRunner

    from python.adversarial_metrics_batch import print_summary_table

    rows = [
        RepoMetricsRow("a", "repos", "3", "0.1200", "0.8500"),
        RepoMetricsRow("b", "foil", "0", "n/a", "n/a"),
    ]

    @click.command()
    def show() -> None:
        print_summary_table(rows)

    result = CliRunner().invoke(show, [])
    assert result.exit_code == 0
    assert "repo" in result.output
    assert "a" in result.output
    assert "0.1200" in result.output


def test_run_metrics_batch_empty(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    kiss = tmp_path / "kiss"
    kiss.mkdir()
    monkeypatch.setattr(
        "python.adversarial_common.repo_root", lambda: kiss, raising=False
    )
    assert run_metrics_batch() == 0
