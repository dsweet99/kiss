"""Tests for adversarial foil CLI persistence."""

from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial as adv
import python.adversarial_foil_cli as foil_mod
import python.adversarial_loop as adv_loop
from click.testing import CliRunner
from python.adversarial_foil_cli import foil
from python.adversarial_foil_stubs import stub_foil_env


def test_foil_success_moves_repo_to_kiss_adversarial(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, adv_root = stub_foil_env(tmp_path, monkeypatch)
    fixed_id = adv_root / "foil" / "20260604_120000_abcd"
    monkeypatch.setattr(foil_mod, "allocate_adversarial_id", lambda _k: fixed_id)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (True, adv.ParsedMetrics(0.55, 0.3), "mean+std(c_f): 0.5500\n"),
    )

    work_dir = tmp_path / "foil_repo"
    result = CliRunner().invoke(foil, [])
    assert result.exit_code == 0
    assert fixed_id.is_dir()
    assert not work_dir.exists()
    parsed = adv_loop.parse_foil_success_path(result.output)
    assert parsed == fixed_id.resolve()
    assert not Path(f"{work_dir}_prompt.md").exists()


def test_foil_failure_leaves_no_kiss_adversarial_repo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, adv_root = stub_foil_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        adv,
        "verify_foil",
        lambda *_: (False, adv.ParsedMetrics(0.1, 0.9), "mean+std(c_f): 0.1000\n"),
    )

    work_dir = tmp_path / "foil_repo"
    result = CliRunner().invoke(foil, [])
    assert result.exit_code != 0
    foil_dir = adv_root / "foil"
    assert not foil_dir.exists() or list(foil_dir.iterdir()) == []
    assert not work_dir.exists()
    assert not Path(f"{work_dir}_prompt.md").exists()
