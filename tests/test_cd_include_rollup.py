from __future__ import annotations

import json
from pathlib import Path

from ops.cd_include_rollup import build_rust_include_edges, rollup_inc_coverage


def test_rollup_inc_coverage_merges_into_parent(tmp_path: Path) -> None:
    parent = tmp_path / "parent.rs"
    child = tmp_path / "child.inc"
    parent.write_text('include!("child.inc");\n', encoding="utf-8")
    child.write_text("", encoding="utf-8")
    parent_r = parent.resolve()
    child_r = child.resolve()
    edges = {parent_r: [child_r]}
    merged = rollup_inc_coverage({parent_r: 20.0, child_r: 100.0}, edges)
    assert child_r not in merged
    assert merged[parent_r] == 60.0


def test_rollup_inc_coverage_adds_parent_when_missing(tmp_path: Path) -> None:
    parent = tmp_path / "wrap.rs"
    child = tmp_path / "body.inc"
    parent.write_text('include!("body.inc");\n', encoding="utf-8")
    child.write_text("", encoding="utf-8")
    parent_r = parent.resolve()
    child_r = child.resolve()
    edges = {parent_r: [child_r]}
    merged = rollup_inc_coverage({child_r: 80.0}, edges)
    assert child_r not in merged
    assert merged[parent_r] == 80.0


def test_build_rust_include_edges_finds_inc(tmp_path: Path) -> None:
    parent = tmp_path / "lib.rs"
    child = tmp_path / "frag.inc"
    parent.write_text('include!("frag.inc");\n', encoding="utf-8")
    child.write_text("", encoding="utf-8")
    edges = build_rust_include_edges(tmp_path)
    assert parent.resolve() in edges
    assert child.resolve() in edges[parent.resolve()]


def test_run_python_coverage_discrepancy(monkeypatch, tmp_path: Path, capsys) -> None:
    from ops.cd_discrepancy_report import DiscrepancyReport, FileCoverage
    from ops.cd_python_run import PythonCoverageRun, run_python_coverage_discrepancy

    repo = tmp_path / "proj"
    repo.mkdir()
    pair = FileCoverage(repo / "m.py", 100.0, 50.0)
    fake = DiscrepancyReport(
        repo=repo,
        language="python",
        n_files=1,
        kiss_median_pct=100.0,
        runtime_total_pct=50.0,
        global_gap=50.0,
        file_mae=50.0,
        file_max_abs_diff=0.5,
        file_rmse=0.5,
        spearman=1.0,
        inflation_rate=0.0,
        blind_spot_rate=0.0,
        pairs=(pair,),
    )
    monkeypatch.setattr(
        "ops.cd_python_run.slipcover_per_file",
        lambda _repo, _args, source=None: ({}, 50.0),
    )
    monkeypatch.setattr("ops.cd_python_run.analyze", lambda *_a, **_k: fake)
    run_python_coverage_discrepancy(
        PythonCoverageRun(repo, "pkg", ("tests/",), False, tmp_path / "out.json")
    )
    assert not capsys.readouterr().out
    payload = json.loads((tmp_path / "out.json").read_text())
    assert payload["summary"]["file_rmse"] == 0.5
