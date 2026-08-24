
from __future__ import annotations

import runpy
from pathlib import Path


def test_python_coverage_metrics_cli_script_delegates(monkeypatch) -> None:
    import python.coverage_metrics as coverage_metrics

    called = []
    monkeypatch.setattr(
        coverage_metrics,
        "coverage_metrics_cli",
        lambda: called.append("coverage_metrics_cli"),
    )

    runpy.run_path(
        str(Path(__file__).resolve().parents[1] / "python" / "coverage_metrics_cli.py"),
        run_name="__main__",
    )

    assert called == ["coverage_metrics_cli"]
