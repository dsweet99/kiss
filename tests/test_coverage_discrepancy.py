from __future__ import annotations

import json
from pathlib import Path

import click
import ops.coverage_discrepancy as cd
from ops.coverage_discrepancy import (
    DiscrepancyReport,
    FileCoverage,
    RuntimeCoverage,
    analyze,
    emit_report,
    print_detailed_report,
    print_report,
    write_report_json,
)


def _sample_report(tmp_path: Path) -> DiscrepancyReport:
    repo = tmp_path / "proj"
    repo.mkdir()
    pair = FileCoverage(repo / "m.py", 100.0, 80.0)
    return DiscrepancyReport(
        repo=repo,
        language="python",
        n_files=1,
        file_mae=20.0,
        file_rmse=0.2,
        spearman=1.0,
        inflation_rate=0.0,
        blind_spot_rate=0.0,
        kiss_median_pct=100.0,
        runtime_total_pct=80.0,
        global_gap=20.0,
        pairs=(pair,),
    )


def test_analyze_computes_rmse(tmp_path: Path, monkeypatch) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    (repo / "src").mkdir()
    (repo / "src" / "m.py").write_text("def f():\n    pass\n")

    kiss_map = {(repo / "src" / "m.py").resolve(): 100.0}
    runtime_map = {(repo / "src" / "m.py").resolve(): 80.0}

    monkeypatch.setattr(
        "ops.cd_analyze.kiss_per_file", lambda _repo, **kwargs: kiss_map
    )
    monkeypatch.setattr("ops.cd_analyze.kiss_summary_median", lambda _repo: 100.0)

    report = analyze(repo, "python", RuntimeCoverage(runtime_map, 80.0))
    assert report.n_files == 1
    assert report.file_rmse == 0.2


def test_print_and_write_report(tmp_path: Path, capsys) -> None:
    report = _sample_report(tmp_path)
    print_report(report)
    out = capsys.readouterr().out
    assert "file_rmse: 0.200" in out
    print_detailed_report(report)
    detailed = capsys.readouterr().out
    assert "m.py" in detailed
    json_path = tmp_path / "out.json"
    write_report_json(report, json_path)
    payload = json.loads(json_path.read_text())
    assert payload["summary"]["file_rmse"] == 0.2
    emit_report(report, detailed=False, report_out=json_path)
    assert json_path.exists()


def test_discrepancy_report_dataclass_and_run_helper() -> None:
    assert cd.DiscrepancyReport.__name__ == "DiscrepancyReport"
    assert cd.run(["echo", "ok"]) == "ok\n"


def test_register_python_command() -> None:
    import ops.cd_cli as cd_cli_mod

    group = click.Group()
    cd_cli_mod.register_python_command(group)
    assert "python" in group.commands


def test_python_command_dispatches(monkeypatch, tmp_path: Path) -> None:
    import ops.cd_click
    import ops.cd_python_run

    repo = tmp_path / "proj"
    repo.mkdir()
    calls: list[object] = []

    def _capture(run: object) -> None:
        calls.append(run)

    monkeypatch.setattr("ops.cd_python_run.run_python_coverage_discrepancy", _capture)
    from click.testing import CliRunner

    result = CliRunner().invoke(
        ops.cd_click.cli,
        ["python", str(repo)],
    )
    assert result.exit_code == 0, result.output
    assert calls
    assert calls[0].repo == repo


def test_python_command_registered_on_cli() -> None:
    import ops.cd_click
    import ops.cd_cli

    assert "python" in ops.cd_click.cli.commands


def test_cli_group_help(capsys) -> None:
    try:
        cd.cli(["--help"])
    except SystemExit as exc:
        assert exc.code == 0
    help_out = capsys.readouterr().out
    assert "rust" in help_out and "python" in help_out


def test_public_entrypoints_referenced() -> None:
    from ops.cd_analyze import analyze_discrepancy
    from ops.cd_python_run import PythonCoverageRun
    assert callable(cd.main)
    assert callable(cd.rust_cmd)
    assert callable(cd.python_cmd)
    assert callable(analyze_discrepancy)
    assert callable(cd.kiss_per_file)
    assert callable(cd.kiss_summary_median)
    assert callable(cd.llvm_cov_per_file)
    assert callable(cd.slipcover_per_file)
    assert callable(cd.run_python_coverage_discrepancy)
    assert PythonCoverageRun(Path("."), None, ("tests/",), False, None).repo == Path(".")
    from ops.cd_click import report_options

    assert callable(cd.python)
    assert callable(report_options)
