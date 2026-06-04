"""Tests for python.coverage_collect."""

from __future__ import annotations

import os
from pathlib import Path

import pytest

import python.coverage_collect as collect
from coverage_metrics_stubs import CARGO_LLVM_OK, SLIPCOVER_OK, install_path_stub


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
    with pytest.raises(RuntimeError, match="did not write coverage JSON"):
        collect.run_slipcover(repo)


def test_parse_llvm_cov_payload_extracts_files(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    payload = {
        "data": [
            {
                "files": [
                    {"filename": "src/lib.rs", "summary": {"lines": {"percent": 55.5}}},
                    {"filename": "src/lib.rs", "summary": {"lines": {}}},
                ],
            },
        ],
    }
    got = collect.parse_llvm_cov_payload(payload, repo)
    assert got == {"src/lib.rs": 55.5}


def test_run_llvm_cov_parses_json(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "Cargo.toml").write_text("[package]\nname='x'\n", encoding="utf-8")
    bindir = install_path_stub(tmp_path, "cargo", CARGO_LLVM_OK)
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    got = collect.run_llvm_cov(repo)
    assert got == {"src/lib.rs": 55.5}


def test_run_llvm_cov_missing_cargo_raises(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    monkeypatch.setenv("PATH", "")
    with pytest.raises(RuntimeError, match="cargo not found"):
        collect.run_llvm_cov(repo)


def test_run_true_coverage_python_only(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "mod.py").write_text("x=1\n", encoding="utf-8")
    monkeypatch.setattr(collect, "run_slipcover", lambda _repo: {"mod.py": 100.0})
    monkeypatch.setattr(collect, "run_llvm_cov", lambda _repo: {"should_not_run.rs": 0.0})
    got = collect.run_true_coverage(repo)
    assert got == {"mod.py": 100.0}


def test_run_true_coverage_no_sources_raises(tmp_path: Path) -> None:
    repo = tmp_path / "empty"
    repo.mkdir()
    with pytest.raises(RuntimeError, match="no Python or Rust sources"):
        collect.run_true_coverage(repo)
