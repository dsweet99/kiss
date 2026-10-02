
from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial as adv


@pytest.mark.parametrize(
    ("metrics", "expected"),
    [
        (adv.ParsedMetrics(0.5, 0.6), True),
        (adv.ParsedMetrics(0.1, 0.9), True),
        (adv.ParsedMetrics(0.51, 0.9), False),
        (adv.ParsedMetrics(0.1, 0.59), False),
        (adv.ParsedMetrics(None, 0.9), False),
        (adv.ParsedMetrics(0.1, None), False),
        (adv.ParsedMetrics(None, None), False),
    ],
)
def test_metrics_pass(metrics: adv.ParsedMetrics, expected: bool) -> None:
    assert adv.metrics_pass(metrics) is expected


def test_build_fix_prompt_contains_paths_and_thresholds(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    kiss.mkdir()
    repo.mkdir()
    text = adv.build_fix_prompt(kiss, [repo])
    assert str(repo.resolve()) in text
    assert str(kiss.resolve()) in text
    assert "Do **not** modify the counterexample repository" in text
    assert "mean+std(c_f)" in text
    assert "0.5" in text
    assert "0.6" in text


def test_verify_fix_delegates_to_run_coverage_metrics(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        adv,
        "run_coverage_metrics",
        lambda _k, _r: (
            "mean+std(c_f): 0.2000\n"
            "spearman(coverage_true, coverage_kiss): 0.9000\n"
        ),
    )
    ok, metrics, text = adv.verify_fix(tmp_path, tmp_path)
    assert ok is True
    assert metrics.mean_plus_std == pytest.approx(0.2)
    assert metrics.spearman == pytest.approx(0.9)
    assert "0.2000" in text


def test_build_fix_prompt_multiple_repos(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo_a = tmp_path / "a"
    repo_b = tmp_path / "b"
    kiss.mkdir()
    repo_a.mkdir()
    repo_b.mkdir()
    text = adv.build_fix_prompt(kiss, [repo_a, repo_b])
    assert str(repo_a.resolve()) in text
    assert str(repo_b.resolve()) in text
    assert "every repo" in text


def test_verify_fix_fails_on_missing_metrics(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        adv,
        "run_coverage_metrics",
        lambda _k, _r: "mean+std(c_f): 0.2000\n",
    )
    ok, metrics, _text = adv.verify_fix(tmp_path, tmp_path)
    assert ok is False
    assert metrics.spearman is None

