"""Tests for adversarial loop orchestration."""

from __future__ import annotations

from pathlib import Path

import pytest
import python.adversarial_loop as adv_loop

_LOOP_CALLS: list[list[str]] = []


def _loop_runner_foil_then_fix(cmd: list[str], _cwd: Path) -> tuple[int, str]:
    _LOOP_CALLS.append(cmd)
    if "foil" in cmd:
        return 0, "foil success: /tmp/repo1\n"
    return 0, "fix success: /tmp/repo1\n"


def _loop_runner_foil_fail(cmd: list[str], _cwd: Path) -> tuple[int, str]:
    _LOOP_CALLS.append(cmd)
    return 1, "foil conditions not met\n"


def _loop_runner_foil_no_success(_cmd: list[str], _cwd: Path) -> tuple[int, str]:
    return 0, "foil repo: /tmp/x\n"


def _loop_runner_fix_fail(cmd: list[str], _cwd: Path) -> tuple[int, str]:
    _LOOP_CALLS.append(cmd)
    if "foil" in cmd:
        return 0, "foil success: /tmp/repo\n"
    return 1, "fix conditions not met\n"


def _loop_runner_count_foil(cmd: list[str], _cwd: Path) -> tuple[int, str]:
    if "foil" in cmd:
        n = sum(1 for c in _LOOP_CALLS if "foil" in c) + 1
        _LOOP_CALLS.append(cmd)
        return 0, f"foil success: /tmp/repo{n}\n"
    _LOOP_CALLS.append(cmd)
    return 0, ""


def test_run_adversarial_loop_single_iteration() -> None:
    _LOOP_CALLS.clear()
    script = Path("/fake/adversarial.py")
    adv_loop.run_adversarial_loop(
        adv_loop.AdversarialLoopConfig(script, 1, None, Path("/kiss")),
        run_command=_loop_runner_foil_then_fix,
        log_stderr=lambda _msg: None,
    )
    assert len(_LOOP_CALLS) == 2
    assert _LOOP_CALLS[0][-1] == "foil"
    assert _LOOP_CALLS[1][-2:] == ["fix", "/tmp/repo1"]


def test_run_adversarial_loop_forwards_lang() -> None:
    _LOOP_CALLS.clear()
    script = Path("/fake/adversarial.py")
    adv_loop.run_adversarial_loop(
        adv_loop.AdversarialLoopConfig(script, 1, "rust", Path("/kiss")),
        run_command=_loop_runner_foil_then_fix,
        log_stderr=lambda _msg: None,
    )
    foil_cmd = next(c for c in _LOOP_CALLS if "foil" in c)
    assert "--lang" in foil_cmd
    assert foil_cmd[foil_cmd.index("--lang") + 1] == "rust"


@pytest.mark.parametrize(
    "case",
    [
        (_loop_runner_foil_fail, 3, "foil failed", 1),
        (_loop_runner_foil_no_success, 1, "foil did not emit", 0),
        (_loop_runner_fix_fail, 2, "fix failed", 2),
    ],
)
def test_run_adversarial_loop_errors(
    case: tuple[object, int, str, int],
) -> None:
    runner, num_iterations, match, expected_calls = case
    _LOOP_CALLS.clear()
    with pytest.raises(RuntimeError, match=match):
        adv_loop.run_adversarial_loop(
            adv_loop.AdversarialLoopConfig(
                Path("/fake/adversarial.py"), num_iterations, None, Path("/kiss")
            ),
            run_command=runner,
            log_stderr=lambda _msg: None,
        )
    assert len(_LOOP_CALLS) == expected_calls


def test_run_adversarial_loop_multiple_iterations() -> None:
    _LOOP_CALLS.clear()
    adv_loop.run_adversarial_loop(
        adv_loop.AdversarialLoopConfig(Path("/fake/adversarial.py"), 3, None, Path("/kiss")),
        run_command=_loop_runner_count_foil,
        log_stderr=lambda _msg: None,
    )
    foil_runs = [c for c in _LOOP_CALLS if "foil" in c]
    assert len(foil_runs) == 3
