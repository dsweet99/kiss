"""Additional tests for adversarial metric parsing and prompt building."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
import python.adversarial as adv
import python.adversarial_loop as adv_loop


def test_run_malvin_code_returns_subprocess_rc(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss = tmp_path / "kiss"
    prompt = tmp_path / "prompt.md"
    kiss.mkdir()
    prompt.write_text("prompt\n", encoding="utf-8")
    seen: dict[str, object] = {}

    def fake_run(cmd: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        seen["cmd"] = cmd
        seen["cwd"] = kwargs["cwd"]
        return subprocess.CompletedProcess(cmd, 7, "", "")

    monkeypatch.setattr(adv.subprocess, "run", fake_run)

    assert adv.run_malvin_code(kiss, prompt) == 7
    assert seen["cmd"] == ["malvin", "code", "--tenacious", f"@{prompt}"]
    assert seen["cwd"] == kiss

def test_run_coverage_metrics_combines_stderr(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    (kiss / "ops").mkdir(parents=True)
    repo.mkdir()
    (kiss / "ops" / "coverage_metrics.py").write_text("# stub\n", encoding="utf-8")

    def fake_run(cmd: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        assert cmd[0]
        assert kwargs["cwd"] == kiss
        return subprocess.CompletedProcess(cmd, 1, "out\n", "err\n")

    monkeypatch.setattr(adv.subprocess, "run", fake_run)

    assert adv.run_coverage_metrics(kiss, repo) == "out\n\nerr\n"

def test_run_coverage_metrics_stdout_only(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    kiss = tmp_path / "kiss"
    repo = tmp_path / "repo"
    (kiss / "ops").mkdir(parents=True)
    repo.mkdir()
    (kiss / "ops" / "coverage_metrics.py").write_text("# stub\n", encoding="utf-8")

    def fake_run(cmd: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        assert cmd[-1] == str(repo.resolve())
        assert kwargs["capture_output"] is True
        return subprocess.CompletedProcess(cmd, 0, "out\n", "")

    monkeypatch.setattr(adv.subprocess, "run", fake_run)

    assert adv.run_coverage_metrics(kiss, repo) == "out\n"

def test_verify_foil_and_fix_use_parsed_metrics(monkeypatch: pytest.MonkeyPatch) -> None:
    output = (
        "mean+std(c_f): 0.4\n"
        "spearman(coverage_true, coverage_kiss): 0.7\n"
    )
    monkeypatch.setattr(adv, "run_coverage_metrics", lambda _k, _r: output)

    foil_ok, foil_metrics, foil_output = adv.verify_foil(Path("/kiss"), Path("/repo"))
    fix_ok, fix_metrics, fix_output = adv.verify_fix(Path("/kiss"), Path("/repo"))

    assert foil_ok is False
    assert fix_ok is True
    assert foil_metrics == fix_metrics == adv.ParsedMetrics(0.4, 0.7)
    assert foil_output == fix_output == output


@pytest.mark.parametrize(
    ("text", "expected"),
    [
        ("no success line\n", None),
        (
            "foil repo: /tmp/kiss_foil_abc\nfoil success: /tmp/kiss_foil_abc\n",
            Path("/tmp/kiss_foil_abc"),
        ),
        (
            "foil success: /tmp/first\nnoise\nfoil success: /tmp/second\n",
            Path("/tmp/second"),
        ),
        (
            "echo foil success: /fake\nfoil success: /tmp/real\n",
            Path("/tmp/real"),
        ),
        ("foil success: /tmp/path with spaces\n", Path("/tmp/path with spaces")),
    ],
)
def test_parse_foil_success_path(text: str, expected: Path | None) -> None:
    assert adv_loop.parse_foil_success_path(text) == expected
