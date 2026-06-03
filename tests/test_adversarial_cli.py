"""Tests for ops.adversarial CLI."""

from __future__ import annotations

from pathlib import Path

import pytest
from click.testing import CliRunner

import ops.adversarial as cli
import python.adversarial as adv
from ops.adversarial import foil, main


def test_main_help() -> None:
    runner = CliRunner()
    result = runner.invoke(main, ["--help"])
    assert result.exit_code == 0
    assert "foil" in result.output
    assert "fix" in result.output
    assert main.__doc__ is not None


def test_repo_root_points_at_kiss() -> None:
    root = cli._repo_root()
    assert (root / "ops" / "adversarial.py").is_file()


def test_ensure_import_path_inserts_repo_root() -> None:
    import sys

    root = str(cli._repo_root())
    sys.path[:] = [p for p in sys.path if p != root]
    cli._ensure_import_path()
    assert root in sys.path


def _stub_foil_env(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> Path:
    kiss = tmp_path / "kiss"
    ops_dir = kiss / "ops"
    ops_dir.mkdir(parents=True)
    (ops_dir / "coverage_metrics.py").write_text("# stub\n", encoding="utf-8")

    def fake_mkdtemp(*, prefix: str, dir: str) -> str:
        repo = tmp_path / "foil_repo"
        repo.mkdir(exist_ok=True)
        return str(repo)

    monkeypatch.setattr(cli.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(cli, "_repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)
    return kiss


def test_foil_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _stub_foil_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (True, adv.ParsedMetrics(0.55, 0.3), "mean+std(c_f): 0.5500\n"),
    )

    runner = CliRunner()
    result = runner.invoke(foil, [])
    assert result.exit_code == 0
    assert "foil success:" in result.output


def test_foil_cli_failure_when_not_violated(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _stub_foil_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (False, adv.ParsedMetrics(0.1, 0.9), "mean+std(c_f): 0.1000\n"),
    )

    runner = CliRunner()
    result = runner.invoke(foil, [])
    assert result.exit_code != 0
    assert "foil conditions not met" in result.output
