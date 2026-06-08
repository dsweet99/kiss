"""Tests for lazy adversarial Click group."""

from __future__ import annotations

import click

from ops.adversarial_group import AdversarialGroup


def test_list_commands_returns_sorted_names() -> None:
    group = AdversarialGroup(
        {"b": ("m", "c"), "a": ("m", "d")},
        lambda _n: None,
        help="test group",
    )
    ctx = click.Context(group)
    assert group.list_commands(ctx) == ["a", "b"]


def test_adversarial_group___init___stores_specs() -> None:
    group = object.__new__(AdversarialGroup)
    AdversarialGroup.__init__(
        group,
        {"foil": ("ops.adversarial_foil", "foil")},
        lambda _n: None,
        help="adversarial",
    )
    assert group._specs == {"foil": ("ops.adversarial_foil", "foil")}


def test_get_command_uses_loader() -> None:
    seen: list[str] = []

    def loader(name: str) -> click.Command | None:
        seen.append(name)
        return None

    group = AdversarialGroup({"foil": ("m", "f")}, loader)
    ctx = click.Context(group)
    assert group.get_command(ctx, "foil") is None
    assert seen == ["foil"]
    assert group.get_command(ctx, "missing") is None
