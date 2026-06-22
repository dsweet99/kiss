"""Tests for adversarial cheat command and verification."""

from __future__ import annotations

import subprocess
from pathlib import Path

import ops.adversarial_cheat as cheat_cli
import pytest
import python.adversarial_cheat as cheat_mod
from click.testing import CliRunner
from ops.adversarial_cheat import cheat_verify


@pytest.mark.parametrize(
    ("kiss_partial", "true", "expected_gaps"),
    [
        ({}, {"src/a.py": 10.0, "tests/test_a.py": 100.0}, [("src/a.py", 100.0, 10.0)]),
        ({"src/a.py": 100.0}, {"src/a.py": 79.0}, [("src/a.py", 100.0, 79.0)]),
        ({"src/a.py": 50.0}, {"src/a.py": 10.0}, []),
        ({}, {"src/a.py": 90.0}, []),
        ({}, {"tests/test_a.py": 10.0}, []),
    ],
)
def test_cheat_gaps(
    kiss_partial: dict[str, float],
    true: dict[str, float],
    expected_gaps: list[tuple[str, float, float]],
) -> None:
    assert cheat_mod.cheat_gaps(kiss_partial, true) == expected_gaps


@pytest.mark.parametrize(
    ("metrics", "expected"),
    [
        (cheat_mod.CheatMetrics(True, (("src/a.py", 100.0, 10.0),)), True),
        (cheat_mod.CheatMetrics(True, ()), False),
        (cheat_mod.CheatMetrics(False, (("src/a.py", 100.0, 10.0),)), False),
    ],
)
def test_cheat_satisfied(metrics: cheat_mod.CheatMetrics, expected: bool) -> None:
    assert cheat_mod.cheat_satisfied(metrics) is expected


def test_build_cheat_prompt_contains_paths_and_thresholds(tmp_path: Path) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    kiss.mkdir()
    (kiss / "ops").mkdir()
    (kiss / "ops" / "adversarial.py").write_text("# stub\n", encoding="utf-8")
    repo.mkdir()
    text = cheat_mod.build_cheat_prompt(kiss, repo, "python")
    assert str(repo.resolve()) in text
    assert "cheat-verify" in text
    assert "below 80%" in text


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


def test_cheat_report_and_verify(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()

    def fake_run(cmd: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        assert cmd[0] == "kiss"
        return subprocess.CompletedProcess(cmd, 0, "ok\n", "")

    monkeypatch.setattr(cheat_mod.subprocess, "run", fake_run)
    code, text = cheat_mod.run_kiss_check(repo)
    assert code == 0
    assert text == "ok\n"

    metrics = cheat_mod.CheatMetrics(True, (("src/a.py", 100.0, 12.0),))
    report = cheat_mod.format_cheat_report(metrics)
    assert "kiss check: pass" in report
    assert "cheat gap count: 1" in report

    monkeypatch.setattr(cheat_mod, "run_kiss_check", lambda _r: (0, "kiss ok\n"))
    monkeypatch.setattr(
        cheat_mod,
        "_load_coverage_maps",
        lambda _r: ({}, {"src/a.py": 5.0}),
    )
    ok, verified, combined = cheat_mod.verify_cheat(tmp_path, repo)
    assert ok is True
    assert verified.kiss_passes is True
    assert "cheat gap count: 1" in combined


@pytest.mark.parametrize("expect_code", [0, 1])
def test_cheat_verify_cli(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, expect_code: int
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    passed = expect_code == 0
    output = f"cheat gap count: {1 if passed else 0}\n"
    monkeypatch.setattr(cheat_cli, "repo_root", lambda: tmp_path)
    monkeypatch.setattr(
        cheat_mod,
        "verify_cheat",
        lambda *_: (passed, cheat_mod.CheatMetrics(passed, ()), output),
    )

    result = CliRunner().invoke(cheat_verify, [str(repo)])
    assert result.exit_code == expect_code
    assert output.strip() in result.output
