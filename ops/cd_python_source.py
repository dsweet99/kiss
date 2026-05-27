from __future__ import annotations

import tomllib
from pathlib import Path


def infer_slipcover_source(repo: Path) -> str | None:
    """Guess slipcover --source for a Python repo when the CLI flag is omitted."""
    repo = repo.resolve()
    from_pyproject = _source_from_pyproject(repo)
    if from_pyproject is not None:
        return from_pyproject
    return _source_from_package_dirs(repo)


def _source_from_pyproject(repo: Path) -> str | None:
    pyproject = repo / "pyproject.toml"
    if not pyproject.is_file():
        return None
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    tool = data.get("tool", {})
    if isinstance(tool, dict):
        poetry = tool.get("poetry", {})
        if isinstance(poetry, dict):
            packages = poetry.get("packages", [])
            if isinstance(packages, list) and packages:
                first = packages[0]
                if isinstance(first, dict) and "include" in first:
                    include = first["include"]
                    if isinstance(include, str):
                        return include.split("/")[0]
    project = data.get("project", {})
    if isinstance(project, dict):
        name = project.get("name")
        if isinstance(name, str) and (repo / name).is_dir():
            return name
    return None


def _source_from_package_dirs(repo: Path) -> str | None:
    skip = {
        "tests",
        "test",
        "testing",
        "docs",
        "doc",
        "scripts",
        "tools",
        "examples",
        "example",
        "benchmarks",
        ".github",
    }
    candidates: list[str] = []
    for child in sorted(repo.iterdir()):
        if not child.is_dir() or child.name.startswith(".") or child.name in skip:
            continue
        if (child / "__init__.py").is_file():
            candidates.append(child.name)
    if len(candidates) == 1:
        return candidates[0]
    return None
