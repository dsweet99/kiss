"""Tests for ops.adversarial CLI."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest
from click.testing import CliRunner

import python.adversarial_common as cli
import ops.adversarial_foil as foil_mod
import python.adversarial as adv
from ops.adversarial import main
from ops.adversarial_foil import foil


def test_main_help() -> None:
    script = cli.repo_root() / "ops" / "run_adversarial.py"
    result = subprocess.run(
        [sys.executable, str(script), "--help"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0
    assert "foil" in result.stdout
    assert "fix" in result.stdout
    assert "cheat" in result.stdout
    assert "loop" in result.stdout


def test_main_group() -> None:
    runner = CliRunner()
    result = runner.invoke(main, ["--help"])
    assert result.exit_code == 0
    assert main.__doc__ is not None


def test_repo_root_points_at_kiss() -> None:
    root = cli.repo_root()
    assert (root / "ops" / "adversarial.py").is_file()


def test_ensure_import_path_inserts_repo_root() -> None:
    root = str(cli.repo_root())
    sys.path[:] = [p for p in sys.path if p != root]
    cli.ensure_import_path()
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

    monkeypatch.setattr(foil_mod.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(foil_mod, "repo_root", lambda: kiss)
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
