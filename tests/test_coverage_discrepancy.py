from __future__ import annotations

import json
from pathlib import Path

from ops.cd_analyze import RuntimeCoverage, analyze_discrepancy as analyze
from ops.cd_discrepancy_report import DiscrepancyReport, FileCoverage
from ops.cd_report_io import emit_report, print_detailed_report, print_report, write_report_json
from ops.cd_runtime import run


def _sample_report(tmp_path: Path) -> DiscrepancyReport:
    repo = tmp_path / "proj"
    repo.mkdir()
    pair = FileCoverage(repo / "m.py", 100.0, 80.0)
    return DiscrepancyReport(
        repo=repo,
        language="python",
        n_files=1,
        file_mae=20.0,
        file_max_abs_diff=0.2,
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
    assert report.file_max_abs_diff == 0.2
    assert report.file_rmse == 0.2


def test_print_and_write_report(tmp_path: Path, capsys) -> None:
    report = _sample_report(tmp_path)
    print_report(report)
    out = capsys.readouterr().out
    assert "file_mae: 20.0" in out
    assert "file_max_abs_diff: 0.200" in out
    assert "file_rmse: 0.200" in out
    print_detailed_report(report)
    detailed = capsys.readouterr().out
    assert "m.py" in detailed
    json_path = tmp_path / "out.json"
    write_report_json(report, json_path)
    payload = json.loads(json_path.read_text())
    assert payload["summary"]["file_max_abs_diff"] == 0.2
    assert payload["summary"]["file_rmse"] == 0.2
    emit_report(report, detailed=False, report_out=json_path)
    payload = json.loads(json_path.read_text())
    assert "files" not in payload
    emit_report(report, detailed=True, report_out=json_path)
    payload = json.loads(json_path.read_text())
    assert payload["files"][0]["file"] == "m.py"


def test_discrepancy_report_dataclass_and_run_helper() -> None:
    assert DiscrepancyReport.__name__ == "DiscrepancyReport"
    assert run(["echo", "ok"]) == "ok\n"


def _invoke_rust_cmd(
    monkeypatch, cd_cli_mod, repo: Path, *, report_out: Path
) -> None:
    rust_calls: list[object] = []
    sample = DiscrepancyReport(
        repo=repo,
        language="rust",
        n_files=0,
        file_mae=0.0,
        file_max_abs_diff=0.0,
        file_rmse=0.0,
        spearman=0.0,
        inflation_rate=0.0,
        blind_spot_rate=0.0,
        kiss_median_pct=0.0,
        runtime_total_pct=0.0,
        global_gap=0.0,
        pairs=(),
    )
    monkeypatch.setattr(cd_cli_mod, "llvm_cov_per_file", lambda _repo: ({}, 0.0))
    monkeypatch.setattr(cd_cli_mod, "analyze", lambda _r, _lang, _runtime: sample)
    monkeypatch.setattr(
        cd_cli_mod, "emit_report", lambda report, **_: rust_calls.append(report.repo)
    )
    cd_cli_mod.rust_cmd.callback(
        repo=repo, detailed=False, report_out=report_out
    )
    assert rust_calls == [repo]


def test_ops_cli_and_runtime_entrypoints(
    monkeypatch, tmp_path: Path, capsys
) -> None:
    import click
    import ops.cd_cli as cd_cli_mod
    import ops.cd_click
    import ops.cd_runtime_kiss as kiss_mod
    import pytest
    from click.testing import CliRunner

    repo = tmp_path / "proj"
    repo.mkdir()
    monkeypatch.setattr(kiss_mod, "KISS_ROOT", tmp_path)
    with pytest.raises(RuntimeError, match="kiss-coverage-map"):
        kiss_mod.kiss_per_file(tmp_path, language="rust")

    calls: list[object] = []
    monkeypatch.setattr("ops.cd_python_run.run_python_coverage_discrepancy", calls.append)
    report_path = tmp_path / "out.json"
    result = CliRunner().invoke(
        ops.cd_click.cli,
        ["python", str(repo.resolve()), "--report-out", str(report_path)],
    )
    assert result.exit_code == 0, result.output
    assert not result.output
    assert calls

    _invoke_rust_cmd(monkeypatch, cd_cli_mod, repo, report_out=tmp_path / "rust.json")

    group = click.Group()
    cd_cli_mod.register_python_command(group)
    assert "python" in group.commands

    def _boom(**_kwargs: object) -> None:
        raise RuntimeError("boom")

    monkeypatch.setattr(cd_cli_mod, "cli", _boom)
    with pytest.raises(SystemExit) as exc:
        cd_cli_mod.main()
    assert exc.value.code == 1
    monkeypatch.setattr(cd_cli_mod, "cli", lambda **kwargs: None)
    cd_cli_mod.main()


def test_python_coverage_command_invoke(monkeypatch, tmp_path: Path) -> None:
    import click
    import ops.cd_cli as cd_cli_mod

    repo = tmp_path / "proj"
    repo.mkdir()
    seen: list[object] = []
    monkeypatch.setattr(cd_cli_mod, "python_cmd", lambda run: seen.append(run))
    monkeypatch.setattr(cd_cli_mod, "infer_pytest_target", lambda _repo: "tests/")
    cmd = cd_cli_mod._PythonCoverageCommand("python", params=cd_cli_mod._python_coverage_params())
    ctx = click.Context(cmd)
    ctx.params = {
        "repo": repo,
        "source": None,
        "pytest_args": (),
        "detailed": False,
        "report_out": tmp_path / "report.json",
    }
    assert cmd.invoke(ctx) == 0
    assert seen and seen[0].repo == repo


def test_interval_audit_rmse(monkeypatch, tmp_path: Path) -> None:
    import math

    import ops.cd_interval_audit as audit_mod
    from ops.cd_interval_audit import IntervalAudit, audit_python_repo

    assert math.isnan(audit_mod._rmse([]))
    pairs = [(100.0, 80.0), (90.0, 85.0)]
    assert abs(audit_mod._rmse(pairs) - 0.145773) < 1e-3

    repo = tmp_path / "proj"
    repo.mkdir()
    mod = (repo / "mod.py").resolve()
    (repo / "mod.py").write_text("def f():\n    pass\n")
    monkeypatch.setattr(audit_mod, "infer_slipcover_source", lambda _repo: "proj")
    monkeypatch.setattr(
        audit_mod,
        "slipcover_per_file",
        lambda _repo, _args, source=None: ({mod: 75.0}, 75.0),
    )
    monkeypatch.setattr(
        audit_mod,
        "_kiss_map",
        lambda _repo, language, bound_flag=None: {
            None: {mod: 100.0},
            "--attested": {mod: 100.0},
            "--optimistic": {mod: 35.0},
        }[bound_flag],
    )
    result = audit_python_repo(repo)
    assert isinstance(result, IntervalAudit)
    assert result.n_files == 1
    assert result.wide_interval_rate == 1.0


def test_interval_audit_main_and_cd_cli_main(monkeypatch, tmp_path: Path, capsys) -> None:
    import ops.cd_interval_audit as audit_mod
    from ops.cd_cli import main as cd_cli_main
    from ops.cd_interval_audit import IntervalAudit, main as interval_audit_main

    repo = tmp_path / "proj"
    repo.mkdir()
    sample = IntervalAudit(
        repo=repo,
        n_files=0,
        outside_rate=float("nan"),
        wide_interval_rate=float("nan"),
        shipped_rmse=float("nan"),
        attested_rmse=float("nan"),
        optimistic_rmse=float("nan"),
        outside_inflated=0,
        outside_blind=0,
        outside_neutral=0,
    )
    monkeypatch.setattr(audit_mod, "audit_python_repo", lambda _repo: sample)
    assert interval_audit_main(["cd_interval_audit.py"]) == 2
    assert interval_audit_main(["cd_interval_audit.py", str(repo)]) == 0
    assert repo.name in capsys.readouterr().out
    assert callable(cd_cli_main)

