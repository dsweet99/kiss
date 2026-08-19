"""Tests for python.adversarial_cli group wiring."""

from __future__ import annotations

import subprocess
import sys

import python.adversarial_cli as adversarial_cli
import python.adversarial_common as cli


def test_main_help() -> None:
    script = cli.repo_root() / "python" / "adversarial_cli.py"
    result = subprocess.run(
        [sys.executable, str(script), "--help"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0
    assert "foil" in result.stdout
    assert "fix" in result.stdout
    assert "fix-cheat" in result.stdout
    assert "cheat" in result.stdout
    assert "loop" in result.stdout
    assert "metrics" in result.stdout


def test_main_lazy_loads_subcommand_help() -> None:
    script = cli.repo_root() / "python" / "adversarial_cli.py"
    result = subprocess.run(
        [sys.executable, str(script), "metrics", "--help"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0
    assert "calibration manifest" in result.stdout


def test_repo_root_points_at_kiss() -> None:
    root = cli.repo_root()
    assert (root / "python" / "adversarial_cli.py").is_file()


def test_ensure_import_path_inserts_repo_root() -> None:
    root = str(cli.repo_root())
    sys.path[:] = [p for p in sys.path if p != root]
    cli.ensure_import_path()
    assert root in sys.path


def test_adversarial_cli_bootstrap_import_path() -> None:
    root = str(cli.repo_root())
    sys.path[:] = [p for p in sys.path if p != root]
    adversarial_cli._bootstrap_import_path()
    assert root in sys.path
