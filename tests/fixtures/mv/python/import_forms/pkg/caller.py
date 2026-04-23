import pkg.source as source_mod
from pkg import source
from pkg.source import exported_fn


def run() -> tuple[int, int, int]:
    return (
        exported_fn(4),
        source_mod.exported_fn(5),
        source.exported_fn(6),
    )
