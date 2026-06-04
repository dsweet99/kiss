#!/usr/bin/env python3
"""Adversarial ops: find kiss coverage counterexamples via malvin."""

from __future__ import annotations

import click

from ops.adversarial_cheat import cheat, cheat_verify
from ops.adversarial_fix import fix
from ops.adversarial_foil import foil
from ops.adversarial_loop import loop


@click.group()
def main() -> None:
    """Adversarial calibration utilities."""


for _command in (foil, fix, cheat, cheat_verify, loop):
    main.add_command(_command)
