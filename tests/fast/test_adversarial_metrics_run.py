
from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

import pytest
from python.adversarial_metrics_batch import run_metrics_batch
from python.adversarial_metrics_stubs import (
    print_good_comparison_output,
    stub_metrics_manifest,
)


def test_run_metrics_batch_discovers_all_manifest_entries(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub_metrics_manifest(tmp_path, monkeypatch)
    calls: list[Path] = []

    def fake_run(repo: Path) -> None:
        calls.append(repo)
        print_good_comparison_output()

    with patch("python.coverage_metrics.run_comparison", fake_run):
        assert run_metrics_batch() == 0

    assert len(calls) == 3
    assert "=== summary ===" in capsys.readouterr().out


def test_run_metrics_batch_prints_per_repo_output_and_summary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub_metrics_manifest(tmp_path, monkeypatch)

    with patch(
        "python.coverage_metrics.run_comparison",
        lambda _r: print_good_comparison_output(),
    ):
        assert run_metrics_batch() == 0

    out = capsys.readouterr().out
    assert "=== " in out
    assert "files compared: 3" in out
    assert "real" in out
    assert "0.1200" in out
    assert "0.8500" in out


def test_run_metrics_batch_summary_shows_na_for_empty_overlap(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub_metrics_manifest(tmp_path, monkeypatch)

    def fake_run(_repo: Path) -> None:
        print("No overlapping files between runtime coverage and kiss analysis.")

    with patch("python.coverage_metrics.run_comparison", fake_run):
        assert run_metrics_batch() == 0

    assert "n/a" in capsys.readouterr().out


def test_run_metrics_batch_exit_nonzero_when_one_repo_fails(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub_metrics_manifest(tmp_path, monkeypatch)
    calls = 0

    def fake_run(_repo: Path) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            raise RuntimeError("measure failed")
        print_good_comparison_output()

    with patch("python.coverage_metrics.run_comparison", fake_run):
        assert run_metrics_batch() == 1

    captured = capsys.readouterr()
    assert "measure failed" in captured.out + captured.err
    assert calls == 3
