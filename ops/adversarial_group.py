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
        self.list_commands = lambda _ctx: sorted(self._specs)
        self.get_command = lambda _ctx, name: (
            self._loader(name) if name in self._specs else None
        )
