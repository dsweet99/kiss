"""Tests for adversarial loop CLI."""

from __future__ import annotations

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
