"""Imports an internal submodule via dotted import."""

import pkg1.submod


def use_pkg1() -> str:
    return pkg1.submod.greet()

