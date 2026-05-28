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
    if not isinstance(name, str):
        return None
    if (repo / name).is_dir():
        return name
    src_name = repo / "src" / name
    if src_name.is_dir():
        return f"src/{name}"
    return None


def _maturin_python_source(data: dict, repo: Path) -> str | None:
    tool = data.get("tool")
    if not isinstance(tool, dict):
        return None
    maturin = tool.get("maturin")
    if not isinstance(maturin, dict):
        return None
    python_source = maturin.get("python-source")
    if not isinstance(python_source, str):
        return None
    module_name = maturin.get("module-name")
    dotted_root = (
        f"{python_source}/{module_name.split('.', 1)[0]}"
        if isinstance(module_name, str) and "." in module_name
        else None
    )
    if dotted_root is not None and (repo / dotted_root).is_dir():
        return dotted_root
    return python_source if (repo / python_source).is_dir() else None


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
    return (
        _maturin_python_source(data, repo)
        or _poetry_include_root(data)
        or _project_name_dir(repo, data)
    )


def infer_pytest_target(repo: Path) -> str:
    """Default pytest path when coverage_discrepancy is invoked without extra args."""
    for candidate in ("tests", "test", "ropetest", "testing"):
        if (repo / candidate).is_dir():
            return f"{candidate}/"
    return "tests/"


def _source_from_maturin_layout(repo: Path) -> str | None:
    for candidate in ("python/ruff", "python"):
        path = repo / candidate
        if path.is_dir() and any(path.glob("**/__init__.py")):
            return candidate
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
    return _source_from_maturin_layout(repo)
