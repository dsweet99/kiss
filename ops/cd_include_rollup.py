from __future__ import annotations

import re
from pathlib import Path

_INCLUDE_RE = re.compile(r"""include!\s*\(\s*"([^"]+)"\s*\)""")


def build_rust_include_edges(repo: Path) -> dict[Path, list[Path]]:
    """Map each `.rs` includer to resolved `.inc` children referenced via `include!`."""
    repo = repo.resolve()
    edges: dict[Path, list[Path]] = {}
    for rs in repo.rglob("*.rs"):
        if not rs.is_file():
            continue
        parent = rs.resolve()
        children: list[Path] = []
        try:
            text = rs.read_text(encoding="utf-8")
        except OSError:
            continue
        for match in _INCLUDE_RE.finditer(text):
            child = (rs.parent / match.group(1)).resolve()
            if child.is_file() and child.suffix == ".inc":
                children.append(child)
        if children:
            edges[parent] = children
    return edges


def rollup_inc_coverage(
    per_file: dict[Path, float], edges: dict[Path, list[Path]]
) -> dict[Path, float]:
    """Fold `.inc` file percentages into their includers and drop fragment paths."""
    out = dict(per_file)
    for parent, children in edges.items():
        group: list[float] = []
        if parent in out:
            group.append(out[parent])
        for child in children:
            if child in out:
                group.append(out.pop(child))
        if not group:
            continue
        out[parent] = sum(group) / len(group)
    return out
