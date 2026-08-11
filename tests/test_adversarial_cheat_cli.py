"""Tests for adversarial cheat CLI persistence."""

from __future__ import annotations

from pathlib import Path

import ops.adversarial_cheat as cheat_cli
import pytest
import python.adversarial as adv
import python.adversarial_cheat as cheat_mod
import python.adversarial_common as cli
from click.testing import CliRunner
from ops.adversarial_cheat import cheat


@pytest.mark.parametrize("expect_success", [True, False])
def test_cheat_cli(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, expect_success: bool
) -> None:
    kiss = tmp_path / "kiss"
    (kiss / "ops").mkdir(parents=True)
    (kiss / "ops" / "adversarial.py").write_text("# stub\n", encoding="utf-8")
    adv_root = tmp_path / "kiss-adversarial"
    monkeypatch.setattr(cli, "adversarial_root", lambda: adv_root)

    def fake_mkdtemp(*, prefix: str, dir: str) -> str:
        repo = tmp_path / "cheat_repo"
        repo.mkdir(exist_ok=True)
        return str(repo)

    monkeypatch.setattr(cheat_cli.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(cheat_cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)

    gaps = (("src/a.py", 100.0, 5.0),) if expect_success else ()
    metrics = cheat_mod.CheatMetrics(expect_success, gaps)
    output = "kiss test: pass\n" if expect_success else "kiss test: fail\n"
    monkeypatch.setattr(cheat_mod, "verify_cheat", lambda *_: (expect_success, metrics, output))

    result = CliRunner().invoke(cheat, [])
    assert (result.exit_code == 0) is expect_success
    if expect_success:
        assert "cheat success:" in result.output
    else:
        assert "cheat conditions not met" in result.output


def test_cheat_success_moves_repo_to_kiss_adversarial(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss = tmp_path / "kiss"
    (kiss / "ops").mkdir(parents=True)
    (kiss / "ops" / "adversarial.py").write_text("# stub\n", encoding="utf-8")
    adv_root = tmp_path / "kiss-adversarial"
    monkeypatch.setattr(cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(cli, "adversarial_root", lambda: adv_root)
    fixed_id = adv_root / "cheat" / "20260604_120000_abcd"
    monkeypatch.setattr(cheat_cli, "allocate_adversarial_id", lambda _k: fixed_id)

    def fake_mkdtemp(*, prefix: str, dir: str) -> str:
        repo = tmp_path / "cheat_repo"
        repo.mkdir(exist_ok=True)
        return str(repo)

    monkeypatch.setattr(cheat_cli.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(cheat_cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)
    monkeypatch.setattr(
        cheat_mod,
        "verify_cheat",
        lambda *_: (
            True,
            cheat_mod.CheatMetrics(True, (("src/a.py", 100.0, 5.0),)),
            "kiss test: pass\n",
        ),
    )

    work_dir = tmp_path / "cheat_repo"
    result = CliRunner().invoke(cheat, [])
    assert result.exit_code == 0
    assert fixed_id.is_dir()
    assert not work_dir.exists()
    assert f"cheat success: {fixed_id.resolve()}" in result.output
    assert not Path(f"{work_dir}_prompt.md").exists()


def test_cheat_failure_leaves_no_kiss_adversarial_repo(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss = tmp_path / "kiss"
    (kiss / "ops").mkdir(parents=True)
    adv_root = tmp_path / "kiss-adversarial"
    monkeypatch.setattr(cli, "adversarial_root", lambda: adv_root)

    def fake_mkdtemp(*, prefix: str, dir: str) -> str:
        repo = tmp_path / "cheat_repo"
        repo.mkdir(exist_ok=True)
        return str(repo)

    monkeypatch.setattr(cheat_cli.tempfile, "mkdtemp", fake_mkdtemp)
    monkeypatch.setattr(cheat_cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(adv, "run_malvin_code", lambda *_: 0)
    monkeypatch.setattr(
        cheat_mod,
        "verify_cheat",
        lambda *_: (False, cheat_mod.CheatMetrics(False, ()), "kiss test: fail\n"),
    )

    work_dir = tmp_path / "cheat_repo"
    result = CliRunner().invoke(cheat, [])
    assert result.exit_code != 0
    cheat_dir = adv_root / "cheat"
    assert not cheat_dir.exists() or list(cheat_dir.iterdir()) == []
    assert not work_dir.exists()
    assert not Path(f"{work_dir}_prompt.md").exists()
