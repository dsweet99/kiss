"""Tests for llvm coverage collection in python.coverage_collect."""

from __future__ import annotations

import os
from pathlib import Path

import pytest
import python.coverage_collect as collect
from coverage_metrics_stubs import CARGO_LLVM_OK, install_path_stub


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
