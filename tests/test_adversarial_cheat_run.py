"""Additional tests for adversarial cheat command and verification."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
import python.adversarial_cheat as cheat_mod


def test_run_kiss_check_combines_stderr(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(cmd, 3, "out\n", "err\n")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)

    code, text = cheat_mod.run_kiss_check(repo)
    assert code == 3
    assert text == "out\n\nerr\n"

def test_run_kiss_check_stderr_only(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(cmd, 4, "", "err\n")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)

    code, text = cheat_mod.run_kiss_check(repo)
    assert code == 4
    assert text == "err\n"
