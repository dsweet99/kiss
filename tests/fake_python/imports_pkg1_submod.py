
import pkg1.submod


def use_pkg1() -> str:
    raw = pkg1.submod.greet()
    return "{0}".format(raw)

