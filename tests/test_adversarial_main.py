"""Tests for adversarial CLI entry wiring."""

from __future__ import annotations

import importlib
import os
import subprocess
import sys

import click
import python.adversarial_common as cli
from click.testing import CliRunner
from ops.adversarial import _load_command, main


def test_load_command_imports_metrics() -> None:
    cmd = _load_command("metrics")
    assert cmd is not None
    assert cmd.name == "metrics"


def test_main_group_help_lists_subcommands() -> None:
    result = CliRunner().invoke(main, ["--help"])
    assert result.exit_code == 0
    assert "metrics" in result.output


def test_main_invokes_metrics_help() -> None:
    result = CliRunner().invoke(main, ["metrics", "--help"])
    assert result.exit_code == 0
    assert "calibration manifest" in result.output


def test_adversarial_module_exports_main() -> None:
    mod = importlib.import_module("ops.adversarial")
    assert isinstance(mod.main, click.Group)


def test_adversarial_script_help() -> None:
    script = cli.repo_root() / "ops" / "adversarial.py"
    env = {**os.environ, "PYTHONPATH": str(cli.repo_root())}
    result = subprocess.run(
        [sys.executable, str(script), "--help"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
    )
    assert result.returncode == 0
    assert "metrics" in result.stdout
    assert "foil" in result.stdout


def test_adversarial_script_help_without_pythonpath() -> None:
    script = cli.repo_root() / "ops" / "adversarial.py"
    env = {k: v for k, v in os.environ.items() if k != "PYTHONPATH"}
    result = subprocess.run(
        [sys.executable, str(script), "--help"],
        capture_output=True,
        text=True,
        check=False,
        env=env,
        cwd=str(cli.repo_root()),
    )
    assert result.returncode == 0
    assert "metrics" in result.stdout
