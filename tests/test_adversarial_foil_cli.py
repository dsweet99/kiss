
from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial as adv
from click.testing import CliRunner
from python.adversarial_foil_cli import foil
from python.adversarial_foil_stubs import stub_foil_env


def test_foil_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    stub_foil_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (True, adv.ParsedMetrics(0.55, 0.3), "mean+std(c_f): 0.5500\n"),
    )

    result = CliRunner().invoke(foil, [])
    assert result.exit_code == 0
    assert "foil success:" in result.output


def test_foil_cli_failure_when_not_violated(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    stub_foil_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (False, adv.ParsedMetrics(0.1, 0.9), "mean+std(c_f): 0.1000\n"),
    )

    result = CliRunner().invoke(foil, [])
    assert result.exit_code != 0
    assert "foil conditions not met" in result.output
