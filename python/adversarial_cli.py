from __future__ import annotations

import importlib
import sys
from pathlib import Path

import click


def _bootstrap_import_path() -> None:
    root = str(Path(__file__).resolve().parent.parent)
    if root not in sys.path:
        sys.path.insert(0, root)


_bootstrap_import_path()

_ag = importlib.import_module("python.adversarial_group")
AdversarialGroup = _ag.AdversarialGroup
CommandSpecs = _ag.CommandSpecs

_COMMAND_SPECS: CommandSpecs = {
    "cheat": ("python.adversarial_cheat_cli", "cheat"),
    "cheat-verify": ("python.adversarial_cheat_cli", "cheat_verify"),
    "fix": ("python.adversarial_fix_cli", "fix"),
    "fix-cheat": ("python.adversarial_fix_cheat_cli", "fix_cheat"),
    "fix-cheat-verify": ("python.adversarial_fix_cheat_cli", "fix_cheat_verify"),
    "foil": ("python.adversarial_foil_cli", "foil"),
    "loop": ("python.adversarial_loop_cli", "loop"),
    "metrics": ("python.adversarial_metrics_cli", "metrics"),
}


def _load_command(cmd_name: str) -> click.Command | None:
    spec = _COMMAND_SPECS.get(cmd_name)
    if spec is None:
        return None
    module_name, attr = spec
    module = importlib.import_module(module_name)
    cmd = getattr(module, attr)
    if not isinstance(cmd, click.Command):
        return None
    return cmd


main = AdversarialGroup(
    _COMMAND_SPECS,
    _load_command,
    name="adversarial",
    help="Adversarial calibration utilities.",
)


if __name__ == "__main__":
    main()
