
from __future__ import annotations

from pathlib import Path


def normalize_path(path: str | Path, repo: Path) -> str:
    p = Path(path)
    if not p.is_absolute():
        p = (repo / p).resolve()
    else:
        p = p.resolve()
    try:
        return str(p.relative_to(repo.resolve()))
    except ValueError:
        return str(p)


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return float("nan")
    ordered = sorted(values)
    k = (len(ordered) - 1) * pct / 100.0
    lo = int(k)
    hi = min(lo + 1, len(ordered) - 1)
    if lo == hi:
        return ordered[lo]
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (k - lo)


def _rankdata(values: list[float]) -> list[float]:
    ordered = sorted((value, index) for index, value in enumerate(values))
    ranks = [0.0] * len(values)
    i = 0
    while i < len(ordered):
        j = i
        while j + 1 < len(ordered) and ordered[j + 1][0] == ordered[i][0]:
            j += 1
        avg = (i + j + 2) / 2.0
        for k in range(i, j + 1):
            ranks[ordered[k][1]] = avg
        i = j + 1
    return ranks


def spearman_correlation(xs: list[float], ys: list[float]) -> float:
    if len(xs) != len(ys) or len(xs) < 2:
        return float("nan")
    rx = _rankdata(xs)
    ry = _rankdata(ys)
    mean_x = sum(rx) / len(rx)
    mean_y = sum(ry) / len(ry)
    num = sum((x - mean_x) * (y - mean_y) for x, y in zip(rx, ry, strict=True))
    den_x = sum((x - mean_x) ** 2 for x in rx) ** 0.5
    den_y = sum((y - mean_y) ** 2 for y in ry) ** 0.5
    if den_x == 0.0 or den_y == 0.0:
        return float("nan")
    return num / (den_x * den_y)


def repo_has_python(repo: Path) -> bool:
    if (repo / "pyproject.toml").exists() or (repo / "setup.py").exists():
        return True
    return any(repo.rglob("*.py"))


def repo_has_rust(repo: Path) -> bool:
    return (repo / "Cargo.toml").exists()
