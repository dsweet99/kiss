"""Tests for python.coverage_collect."""

from __future__ import annotations

import os
from pathlib import Path

import pytest
import python.coverage_collect as collect
from coverage_metrics_stubs import SLIPCOVER_OK, install_path_stub


def test_run_slipcover_parses_json(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "pyproject.toml").write_text("[project]\nname='x'\n", encoding="utf-8")
    bindir = install_path_stub(tmp_path, "slipcover", SLIPCOVER_OK)
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    got = collect.run_slipcover(repo)
    assert got == {"pkg/a.py": 80.0}


def test_run_slipcover_missing_binary_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setenv("PATH", "")
    with pytest.raises(RuntimeError, match="slipcover not found"):
        collect.run_slipcover(repo)


def test_run_slipcover_nonzero_exit_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    bindir = install_path_stub(tmp_path, "slipcover", "#!/bin/sh\nexit 2\n")
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    with pytest.raises(RuntimeError, match="slipcover/pytest failed"):
        collect.run_slipcover(repo)


def test_run_slipcover_missing_output_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    bindir = install_path_stub(tmp_path, "slipcover", "#!/bin/sh\nexit 0\n")
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    with pytest.raises(RuntimeError, match="slipcover/pytest failed"):
        collect.run_slipcover(repo)


def test_pytest_targets_reads_pyproject_testpaths(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "pyproject.toml").write_text(
        '[tool.pytest.ini_options]\ntestpaths = ["pkg_tests"]\n',
        encoding="utf-8",
    )
    assert collect.pytest_targets(repo) == ["pkg_tests"]


def test_pytest_targets_reads_string_testpaths_and_bad_toml(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "pyproject.toml").write_text(
        '[tool.pytest.ini_options]\ntestpaths = "unit_tests"\n',
        encoding="utf-8",
    )
    assert collect.pytest_targets(repo) == ["unit_tests"]

    (repo / "pyproject.toml").write_text("[tool.pytest.ini_options\n", encoding="utf-8")
    assert collect.pytest_targets(repo) == []


def test_pytest_targets_defaults_to_tests_directory(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    (repo / "tests").mkdir(parents=True)
    assert collect.pytest_targets(repo) == ["tests"]


def test_pytest_targets_prefers_fast_tests_directory(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    (repo / "tests" / "fast").mkdir(parents=True)
    (repo / "tests" / "slow").mkdir()
    assert collect.pytest_targets(repo) == ["tests/fast"]


def test_slipcover_invocation_sklearn_uses_pyargs(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    (repo / "sklearn" / "__init__.py").parent.mkdir(parents=True)
    (repo / "sklearn" / "__init__.py").write_text("", encoding="utf-8")
    cwd, args = collect.slipcover_invocation(repo)
    assert cwd == Path("/tmp")
    assert "--pyargs" in args and "sklearn.tests" in args


def test_run_slipcover_accepts_nonzero_exit_when_json_present(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "tests").mkdir()
    script = """#!/usr/bin/env python3
import json, sys
from pathlib import Path
out = Path(sys.argv[sys.argv.index("--out") + 1])
out.write_text(json.dumps({"files": {"a.py": {"summary": {"percent_covered": 50.0}}}}))
sys.exit(2)
"""
    bindir = install_path_stub(tmp_path, "slipcover", script)
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    got = collect.run_slipcover(repo)
    assert got == {"a.py": 50.0}
