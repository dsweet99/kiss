"""Imports an internal submodule via dotted import."""

import pkg2.submod


def use_pkg2() -> str:
    raw = pkg2.submod.greet()
    return "{0}".format(raw)

