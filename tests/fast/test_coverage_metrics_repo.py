
from __future__ import annotations

from pathlib import Path

import python.coverage_stats as stats


def test_repo_has_python_pyproject(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    (repo / "pyproject.toml").write_text("[project]\nname='x'\n", encoding="utf-8")
    assert stats.repo_has_python(repo) is True


def test_repo_has_python_setup_py(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    (repo / "setup.py").write_text("from setuptools import setup\n", encoding="utf-8")
    assert stats.repo_has_python(repo) is True


def test_repo_has_python_any_py_file(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    (repo / "script.py").write_text("x = 1\n", encoding="utf-8")
    assert stats.repo_has_python(repo) is True


def test_repo_has_python_false_when_empty(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    assert stats.repo_has_python(repo) is False


def test_repo_has_rust_with_cargo_toml(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    (repo / "Cargo.toml").write_text("[package]\nname='x'\n", encoding="utf-8")
    assert stats.repo_has_rust(repo) is True


def test_repo_has_rust_false_without_cargo(tmp_path: Path) -> None:
    repo = tmp_path / "r"
    repo.mkdir()
    assert stats.repo_has_rust(repo) is False
