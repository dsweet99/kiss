from __future__ import annotations

from pathlib import Path

from ops.cd_python_source import infer_slipcover_source


def test_infer_slipcover_from_poetry_packages(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    (repo / "mypkg").mkdir()
    (repo / "mypkg" / "__init__.py").write_text("")
    (repo / "pyproject.toml").write_text(
        '[tool.poetry]\npackages = [{ include = "mypkg/sub" }]\n'
    )
    assert infer_slipcover_source(repo) == "mypkg"


def test_infer_slipcover_from_project_name_dir(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    (repo / "widgets").mkdir()
    (repo / "pyproject.toml").write_text('[project]\nname = "widgets"\n')
    assert infer_slipcover_source(repo) == "widgets"


def test_infer_slipcover_single_package_dir(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    (repo / "onlypkg").mkdir()
    (repo / "onlypkg" / "__init__.py").write_text("")
    (repo / "tests").mkdir()
    assert infer_slipcover_source(repo) == "onlypkg"


def test_infer_slipcover_ambiguous_returns_none(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    for name in ("a", "b"):
        (repo / name).mkdir()
        (repo / name / "__init__.py").write_text("")
    assert infer_slipcover_source(repo) is None
