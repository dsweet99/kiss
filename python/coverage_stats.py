
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


def repo_has_python(repo: Path) -> bool:
    if (repo / "pyproject.toml").exists() or (repo / "setup.py").exists():
        return True
    return any(repo.rglob("*.py"))


def repo_has_rust(repo: Path) -> bool:
    return (repo / "Cargo.toml").exists()
