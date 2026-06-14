"""Tests for adversarial loop CLI."""

from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial_loop as adv_loop
from click.testing import CliRunner
from ops.adversarial_loop import loop


def test_loop_cli_success(monkeypatch: pytest.MonkeyPatch) -> None:
    seen: dict[str, object] = {}

    def fake_loop(config: adv_loop.AdversarialLoopConfig, **kwargs: object) -> None:
        seen["config"] = config
        log = kwargs["log_stderr"]
        assert callable(log)
        log("loop success: 2 iteration(s)")

    monkeypatch.setattr(adv_loop, "run_adversarial_loop", fake_loop)

    runner = CliRunner()
    result = runner.invoke(loop, ["--num-iterations", "2", "--lang", "python"])
    assert result.exit_code == 0
    assert "loop success:" in result.output
    config = seen["config"]
    assert isinstance(config, adv_loop.AdversarialLoopConfig)
    assert config.num_iterations == 2
    assert config.lang == "python"


def test_loop_cli_propagates_loop_error(monkeypatch: pytest.MonkeyPatch) -> None:
    def fake_loop(_config: adv_loop.AdversarialLoopConfig, **_kwargs: object) -> None:
        raise RuntimeError("foil failed on iteration 1/1 (exit 1)")

    monkeypatch.setattr(adv_loop, "run_adversarial_loop", fake_loop)

    runner = CliRunner()
    result = runner.invoke(loop, [])
    assert result.exit_code != 0
    assert "foil failed" in result.output


def test_run_adversarial_loop_raises_when_foil_output_has_no_success() -> None:
    config = adv_loop.AdversarialLoopConfig(Path("adv.py"), 1, None, Path("/repo"))

    def fake_runner(cmd: list[str], cwd: Path) -> tuple[int, str]:
        assert cmd[1] == "adv.py"
        assert cwd == Path("/repo")
        return 0, "no success marker\n"

    with pytest.raises(RuntimeError, match="foil did not emit"):
        adv_loop.run_adversarial_loop(config, run_command=fake_runner)


def test_run_adversarial_loop_raises_when_fix_fails() -> None:
    config = adv_loop.AdversarialLoopConfig(Path("adv.py"), 1, "rust", Path("/repo"))
    calls: list[list[str]] = []

    def fake_runner(cmd: list[str], _cwd: Path) -> tuple[int, str]:
        calls.append(cmd)
        if cmd[2] == "foil":
            return 0, "foil success: /tmp/generated\n"
        return 9, "fix failed\n"

    with pytest.raises(RuntimeError, match="fix failed"):
        adv_loop.run_adversarial_loop(config, run_command=fake_runner)
    assert calls[0][-2:] == ["--lang", "rust"]
