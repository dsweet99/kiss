"""CLI tests for adversarial fix-cheat command."""

from __future__ import annotations

from pathlib import Path

import pytest
from click.testing import CliRunner

import ops.adversarial_fix_cheat as fix_cheat_cli
import python.adversarial_fix_cheat as fix_cheat_mod
import python.adversarial_fix_cheat_session as fix_cheat_session
import python.adversarial_verify_batch as verify_batch
from ops.adversarial_fix_cheat import fix_cheat, fix_cheat_verify


def _stub_fix_cheat_env(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[Path, Path]:
    kiss = tmp_path / "kiss"
    ops_dir = kiss / "ops"
    ops_dir.mkdir(parents=True)
    (ops_dir / "adversarial.py").write_text("# stub\n", encoding="utf-8")
    repo = tmp_path / "cheat_repo"
    repo.mkdir()
    monkeypatch.setattr(fix_cheat_cli, "repo_root", lambda: kiss)
    monkeypatch.setattr(fix_cheat_session, "run_malvin_code", lambda *_: 0)
    return kiss, repo


def test_run_fix_cheat_session_success(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss, repo = _stub_fix_cheat_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        fix_cheat_session,
        "verify_fix_cheat_repos",
        lambda *_: (True, [], "cheat gap count: 0\n"),
    )
    fix_cheat_session.run_fix_cheat_session(kiss, [repo])


def test_fix_cheat_cli_success(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _kiss, repo = _stub_fix_cheat_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        fix_cheat_session,
        "verify_fix_cheat_repos",
        lambda *_: (True, [], "cheat gap count: 0\n"),
    )
    result = CliRunner().invoke(fix_cheat, [str(repo)])
    assert result.exit_code == 0
    assert "fix-cheat success:" in result.output
    assert Path(f"{repo.resolve()}_fix_cheat_prompt.md").is_file()


def test_fix_cheat_cli_failure(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _kiss, repo = _stub_fix_cheat_env(tmp_path, monkeypatch)
    monkeypatch.setattr(
        fix_cheat_session,
        "verify_fix_cheat_repos",
        lambda *_: (
            False,
            [
                (
                    repo,
                    fix_cheat_mod.FixCheatMetrics((("src/a.py", 100.0, 5.0),), (), True),
                    "cheat gap count: 1\n",
                )
            ],
            "cheat gap count: 1\n",
        ),
    )
    result = CliRunner().invoke(fix_cheat, [str(repo)])
    assert result.exit_code != 0
    assert "fix-cheat conditions not met" in result.output


def test_fix_cheat_cli_malvin_nonzero_still_verifies(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _kiss, repo = _stub_fix_cheat_env(tmp_path, monkeypatch)
    monkeypatch.setattr(fix_cheat_session, "run_malvin_code", lambda *_: 1)
    monkeypatch.setattr(
        fix_cheat_session,
        "verify_fix_cheat_repos",
        lambda *_: (True, [], "cheat gap count: 0\n"),
    )
    result = CliRunner().invoke(fix_cheat, [str(repo)])
    assert result.exit_code == 0
    assert "malvin exited 1" in result.output


def test_fix_cheat_cli_requires_at_least_one_repo() -> None:
    result = CliRunner().invoke(fix_cheat, [])
    assert result.exit_code != 0


@pytest.mark.parametrize("expect_code", [0, 1])
def test_fix_cheat_verify_cli(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, expect_code: int
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    passed = expect_code == 0
    output = f"cheat gap count: {0 if passed else 1}\n"
    monkeypatch.setattr(fix_cheat_cli, "repo_root", lambda: tmp_path)
    monkeypatch.setattr(
        verify_batch,
        "verify_fix_cheat_repos",
        lambda *_: (passed, [], output),
    )
    result = CliRunner().invoke(fix_cheat_verify, [str(repo)])
    assert result.exit_code == expect_code
    assert output.strip() in result.output
