"""Additional tests for adversarial cheat command and verification."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
import python.adversarial_cheat as cheat_mod


def test_load_coverage_maps_parses_subprocess_json(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    payload = '{"kiss": {"src/a.py": 100.0}, "true": {"src/a.py": 5.0}}'

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        assert "coverage_maps_cli.py" in cmd[1]
        return subprocess.CompletedProcess(cmd, 0, payload, "")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)
    kiss_partial, true = cheat_mod._load_coverage_maps(repo)
    assert kiss_partial == {"src/a.py": 100.0}
    assert true == {"src/a.py": 5.0}

def test_load_coverage_maps_subprocess_env_prepends_repo_root(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    captured: dict[str, object] = {}

    def fake_run(cmd: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        captured.update(kwargs)
        payload = '{"kiss": {}, "true": {}}'
        return subprocess.CompletedProcess(cmd, 0, payload, "")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)
    monkeypatch.setenv("PYTHONPATH", "/other")
    cheat_mod._load_coverage_maps(repo)
    env = captured["env"]
    assert isinstance(env, dict)
    root = str(cheat_mod.repo_root())
    assert env["PYTHONPATH"].startswith(root)
    assert "/other" in env["PYTHONPATH"]

def test_load_coverage_maps_raises_subprocess_error(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(cmd, 2, "", "bad maps")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)

    with pytest.raises(RuntimeError, match="bad maps"):
        cheat_mod._load_coverage_maps(repo)

def test_load_coverage_maps_raises_stdout_error(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        return subprocess.CompletedProcess(cmd, 2, "bad maps on stdout", "")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)

    with pytest.raises(RuntimeError, match="bad maps on stdout"):
        cheat_mod._load_coverage_maps(repo)
