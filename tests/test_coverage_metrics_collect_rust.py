"""Additional tests for python.coverage_collect."""

from __future__ import annotations

import os
from pathlib import Path

import pytest
import python.coverage_collect as collect
from coverage_metrics_stubs import install_path_stub


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

def test_parse_llvm_cov_payload_normalizes_files(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    payload = {
        "data": [
            {
                "files": [
                    {
                        "filename": str(repo / "src" / "lib.rs"),
                        "summary": {"lines": {"percent": 77.0}},
                    },
                    {"filename": None, "summary": {"lines": {"percent": 10.0}}},
                ]
            }
        ]
    }
    assert collect.parse_llvm_cov_payload(payload, repo) == {"src/lib.rs": 77.0}

def test_run_llvm_cov_raises_when_output_missing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    bindir = install_path_stub(tmp_path, "cargo", "#!/bin/sh\nexit 0\n")
    monkeypatch.setenv("PATH", f"{bindir}:{os.environ.get('PATH', '')}")
    with pytest.raises(RuntimeError, match="did not write coverage JSON"):
        collect.run_llvm_cov(repo)
