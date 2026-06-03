"""Tests for adversarial fix command and metrics_pass."""

from __future__ import annotations

from pathlib import Path

import pytest
from click.testing import CliRunner

import ops.adversarial as cli
import python.adversarial as adv
from ops.adversarial import fix


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
    text = adv.build_fix_prompt(kiss, repo)
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


def _stub_fix_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, Path]:
    kiss = tmp_path / "kiss"
    ops_dir = kiss / "ops"
    ops_dir.mkdir(parents=True)
    (ops_dir / "coverage_metrics.py").write_text("# stub\n", encoding="utf-8")
    repo = tmp_path / "counterexample"
    repo.mkdir()

    monkeypatch.setattr(cli, "_repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)
    return kiss, repo


def test_fix_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_fix",
        lambda *_: (True, adv.ParsedMetrics(0.1, 0.9), "mean+std(c_f): 0.1000\n"),
    )

    runner = CliRunner()
    result = runner.invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "fix success:" in result.output
    assert (Path(f"{repo.resolve()}_fix_prompt.md")).is_file()


def test_fix_cli_failure_when_not_passed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_fix",
        lambda *_: (False, adv.ParsedMetrics(0.55, 0.3), "mean+std(c_f): 0.5500\n"),
    )

    runner = CliRunner()
    result = runner.invoke(fix, [str(repo)])
    assert result.exit_code != 0
    assert "fix conditions not met" in result.output


def test_fix_cli_malvin_nonzero_still_verifies(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 1)
    monkeypatch.setattr(
        adv,
        "verify_fix",
        lambda *_: (True, adv.ParsedMetrics(0.1, 0.9), "mean+std(c_f): 0.1000\n"),
    )

    runner = CliRunner()
    result = runner.invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "malvin exited 1" in result.output
