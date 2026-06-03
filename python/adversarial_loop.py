"""Adversarial foil → fix loop orchestration."""

from __future__ import annotations

import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import NamedTuple

FOIL_SUCCESS_RE = re.compile(r"^foil success: (.+)$", re.MULTILINE)


class AdversarialLoopConfig(NamedTuple):
    adversarial_script: Path
    num_iterations: int
    lang: str | None
    cwd: Path


def parse_foil_success_path(output: str) -> Path | None:
    matches = FOIL_SUCCESS_RE.findall(output)
    if not matches:
        return None
    return Path(matches[-1])


def run_streaming_command(
    cmd: list[str], *, cwd: Path | None = None
) -> tuple[int, str]:
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=None,
        text=True,
        cwd=cwd,
        bufsize=1,
    )
    captured: list[str] = []
    assert proc.stdout is not None
    for line in proc.stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        captured.append(line)
    return proc.wait(), "".join(captured)


def run_adversarial_loop(
    config: AdversarialLoopConfig,
    *,
    run_command: Callable[[list[str], Path], tuple[int, str]] | None = None,
    log_stderr: Callable[[str], None] | None = None,
) -> None:
    runner = run_command or (
        lambda cmd, root: run_streaming_command(cmd, cwd=root)
    )
    log = log_stderr or (lambda msg: sys.stderr.write(f"{msg}\n"))
    python = sys.executable
    script = config.adversarial_script
    cwd = config.cwd

    for iteration in range(1, config.num_iterations + 1):
        log(f"loop: iteration {iteration}/{config.num_iterations}")

        foil_cmd = [python, str(script), "foil"]
        if config.lang is not None:
            foil_cmd.extend(["--lang", config.lang])

        foil_rc, foil_output = runner(foil_cmd, cwd)
        if foil_rc != 0:
            msg = (
                f"foil failed on iteration {iteration}/{config.num_iterations} "
                f"(exit {foil_rc})"
            )
            raise RuntimeError(msg)

        repo_path = parse_foil_success_path(foil_output)
        if repo_path is None:
            msg = (
                f"foil did not emit foil success: on iteration "
                f"{iteration}/{config.num_iterations}"
            )
            raise RuntimeError(msg)

        fix_cmd = [python, str(script), "fix", str(repo_path)]
        fix_rc, _fix_output = runner(fix_cmd, cwd)
        if fix_rc != 0:
            msg = (
                f"fix failed on iteration {iteration}/{config.num_iterations} "
                f"(exit {fix_rc})"
            )
            raise RuntimeError(msg)

    log(f"loop success: {config.num_iterations} iteration(s)")
