"""Tests for adversarial subprocess integration."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest
import python.adversarial as adv
import python.adversarial_loop as adv_loop

_MALVIN_CMDS: list[list[str]] = []


def _record_malvin_run(*args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
    cmd = args[0]
    assert isinstance(cmd, list)
    _MALVIN_CMDS.append(cmd)
    return subprocess.CompletedProcess(cmd, 0)


def _stub_coverage_run(*args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
    cmd = args[0]
    assert isinstance(cmd, list)
    return subprocess.CompletedProcess(cmd, 0, stdout="mean+std(c_f): 0.2000\n")


def test_run_malvin_code_subprocess(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    _MALVIN_CMDS.clear()
    monkeypatch.setattr(adv.subprocess, "run", _record_malvin_run)
    kiss = tmp_path / "kiss"
    prompt = tmp_path / "prompt.md"
    assert adv.run_malvin_code(kiss, prompt) == 0
    assert _MALVIN_CMDS == [["malvin", "code", "--tenacious", f"@{prompt}"]]


def test_run_coverage_metrics_subprocess(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(adv.subprocess, "run", _stub_coverage_run)
    out = adv.run_coverage_metrics(tmp_path / "kiss", tmp_path / "repo")
    assert "0.2000" in out


def test_verify_foil_delegates_to_run_coverage_metrics(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(
        adv,
        "run_coverage_metrics",
        lambda _k, _r: (
            "mean+std(c_f): 0.5500\n"
            "spearman(coverage_true, coverage_kiss): 0.9000\n"
        ),
    )
    ok, metrics, text = adv.verify_foil(tmp_path, tmp_path)
    assert ok is True
    assert metrics.mean_plus_std == pytest.approx(0.55)
    assert metrics.spearman == pytest.approx(0.9)
    assert "0.5500" in text


def test_run_streaming_command_relay(tmp_path: Path) -> None:
    script = tmp_path / "echo_lines.py"
    script.write_text(
        "import sys\n"
        "for line in ['alpha\\n', 'foil success: /tmp/x\\n']:\n"
        "    sys.stdout.write(line)\n",
        encoding="utf-8",
    )

    rc, captured = adv_loop.run_streaming_command(
        [sys.executable, str(script)], cwd=tmp_path
    )
    assert rc == 0
    assert "foil success: /tmp/x" in captured
    assert adv_loop.parse_foil_success_path(captured) == Path("/tmp/x")
