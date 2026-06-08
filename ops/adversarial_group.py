"""Lazy Click group for adversarial subcommands."""

from __future__ import annotations

from collections.abc import Callable
from typing import TypeAlias

import click

CommandLoader: TypeAlias = Callable[[str], click.Command | None]
CommandSpecs: TypeAlias = dict[str, tuple[str, str]]


class AdversarialGroup(click.Group):
    def __init__(
        self,
        specs: CommandSpecs,
        loader: CommandLoader,
        **attrs: object,
    ) -> None:
        super().__init__(**attrs)
        self._specs = specs
        self._loader = loader

    def list_commands(self, ctx: click.Context) -> list[str]:
        return sorted(self._specs)

    def get_command(
        self, ctx: click.Context, cmd_name: str
    ) -> click.Command | None:
        if cmd_name not in self._specs:
            return None
        return self._loader(cmd_name)
