"""Imports an internal submodule via dotted import."""

import pkg2.submod


def use_pkg2() -> str:
    return pkg2.submod.greet()

