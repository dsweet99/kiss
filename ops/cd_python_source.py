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


def _poetry_include_root(data: dict) -> str | None:
    tool = data.get("tool")
    if not isinstance(tool, dict):
        return None
    poetry = tool.get("poetry")
    if not isinstance(poetry, dict):
        return None
    packages = poetry.get("packages")
    if not isinstance(packages, list) or not packages:
        return None
    first = packages[0]
    if not isinstance(first, dict):
        return None
    include = first.get("include")
    return include.split("/")[0] if isinstance(include, str) else None


def _project_name_dir(repo: Path, data: dict) -> str | None:
    project = data.get("project")
    if not isinstance(project, dict):
        return None
    name = project.get("name")
    if not isinstance(name, str) or not (repo / name).is_dir():
        return None
    return name


def _source_from_pyproject(repo: Path) -> str | None:
    pyproject = repo / "pyproject.toml"
    if not pyproject.is_file():
        return None
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return None
    if not isinstance(data, dict):
        return None
    return _poetry_include_root(data) or _project_name_dir(repo, data)


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
