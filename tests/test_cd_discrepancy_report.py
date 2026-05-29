from __future__ import annotations

from pathlib import Path

from ops.cd_analyze import RuntimeCoverage, analyze_discrepancy
from ops.cd_analyze_inputs import AnalyzeInputs
from ops.cd_discrepancy_report import align_files, analyze, spearman
from ops.cd_file_coverage import FileCoverage


def test_spearman_perfect_rank() -> None:
    xs = [10.0, 20.0, 30.0]
    ys = [15.0, 25.0, 35.0]
    assert spearman(xs, ys) == 1.0


def test_spearman_too_few_points() -> None:
    assert spearman([1.0], [2.0]) is None


def test_spearman_tied_values() -> None:
    xs = [1.0, 1.0, 3.0]
    ys = [2.0, 2.0, 4.0]
    assert spearman(xs, ys) is not None


def test_align_files_intersection() -> None:
    kiss = {Path("/a.py"): 80.0, Path("/b.py"): 50.0}
    runtime = {Path("/a.py"): 70.0, Path("/c.py"): 90.0}
    pairs = align_files(kiss, runtime)
    assert len(pairs) == 1
    assert pairs[0].path == Path("/a.py")
    assert pairs[0].delta == 10.0
    assert pairs[0].flag == ""


def test_file_coverage_flags() -> None:
    inflated = FileCoverage(Path("x.py"), 90.0, 50.0)
    assert inflated.flag == "inflated"
    blind = FileCoverage(Path("y.py"), 30.0, 60.0)
    assert blind.flag == "blind_spot"


def test_analyze_core_computes_rmse(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    inflated = (repo / "inflated.py").resolve()
    blind = (repo / "blind.py").resolve()
    aligned = (repo / "aligned.py").resolve()
    kiss_map = {inflated: 90.0, blind: 30.0, aligned: 50.0}
    runtime_map = {inflated: 50.0, blind: 60.0, aligned: 50.0}
    report = analyze(
        AnalyzeInputs(
            repo=repo,
            language="python",
            runtime_map=runtime_map,
            runtime_total=55.0,
            kiss_map=kiss_map,
            kiss_median=50.0,
        )
    )
    assert report.n_files == 3
    assert report.inflation_rate == 1 / 3
    assert report.blind_spot_rate == 1 / 3
    assert report.spearman is not None
    assert report.global_gap == abs(50.0 - 55.0)
    assert report.file_mae > 0.0
    assert report.file_max_abs_diff == 0.4
    assert report.file_rmse > 0.0


def test_analyze_discrepancy_wrapper(tmp_path: Path, monkeypatch) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    p = (repo / "m.py").resolve()
    monkeypatch.setattr(
        "ops.cd_analyze.kiss_per_file", lambda _repo, **kwargs: {p: 100.0}
    )
    monkeypatch.setattr("ops.cd_analyze.kiss_summary_median", lambda _repo: 100.0)
    report = analyze_discrepancy(
        repo, "python", RuntimeCoverage({p: 80.0}, 80.0)
    )
    assert report.file_rmse == 0.2


def test_analyze_core_no_overlap_raises(tmp_path: Path) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    try:
        analyze(
            AnalyzeInputs(
                repo=repo,
                language="python",
                runtime_map={Path("/a.py"): 50.0},
                runtime_total=50.0,
                kiss_map={Path("/b.py"): 50.0},
                kiss_median=50.0,
            )
        )
    except RuntimeError as exc:
        assert "no overlapping" in str(exc)
    else:
        raise AssertionError("expected RuntimeError")
