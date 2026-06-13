"""CLI tests for adversarial fix command."""

from __future__ import annotations

from pathlib import Path

import pytest
from click.testing import CliRunner

import ops.adversarial_fix as fix_mod
from ops.adversarial_fix import fix
from python.adversarial import ParsedMetrics


def _stub_fix_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> tuple[Path, Path]:
    kiss = tmp_path / "kiss"
    ops_dir = kiss / "ops"
    ops_dir.mkdir(parents=True)
    (ops_dir / "coverage_metrics.py").write_text("# stub\n", encoding="utf-8")
    repo = tmp_path / "counterexample"
    repo.mkdir()

    monkeypatch.setattr(fix_mod, "repo_root", lambda: kiss)
    monkeypatch.setattr("python.adversarial.run_malvin_code", lambda *_: 0)
    return kiss, repo


def test_fix_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        "python.adversarial_verify_batch.verify_fix_repos",
        lambda *_: (True, [], "mean+std(c_f): 0.1000\n"),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "fix success:" in result.output
    assert Path(f"{repo.resolve()}_fix_prompt.md").is_file()


def test_fix_cli_failure_when_not_passed(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        "python.adversarial_verify_batch.verify_fix_repos",
        lambda *_: (
            False,
            [(repo, ParsedMetrics(0.55, 0.3), "mean+std(c_f): 0.5500\n")],
            "mean+std(c_f): 0.5500\n",
        ),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code != 0
    assert "fix conditions not met" in result.output


def test_fix_cli_malvin_nonzero_still_verifies(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_env(tmp_path, monkeypatch)
    monkeypatch.setattr("python.adversarial.run_malvin_code", lambda *_: 1)
    monkeypatch.setattr(
        "python.adversarial_verify_batch.verify_fix_repos",
        lambda *_: (True, [], "mean+std(c_f): 0.1000\n"),
    )

    result = CliRunner().invoke(fix, [str(repo)])
    assert result.exit_code == 0
    assert "malvin exited 1" in result.output


def test_fix_cli_requires_at_least_one_repo() -> None:
    result = CliRunner().invoke(fix, [])
    assert result.exit_code != 0
