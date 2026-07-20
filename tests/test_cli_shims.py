"""In-process coverage for small CLI shim modules."""

from __future__ import annotations

import runpy
import sys
from pathlib import Path


def test_ops_run_adversarial_run_delegates(monkeypatch) -> None:
    import ops.adversarial as adversarial
    import ops.run_adversarial as run_adversarial

    called = []
    monkeypatch.setattr(adversarial, "main", lambda: called.append("main"))

    run_adversarial._run()

    assert called == ["main"]
    assert str(Path(__file__).resolve().parents[1]) in sys.path


def test_ops_coverage_metrics_script_delegates(monkeypatch) -> None:
    import python.coverage_metrics as coverage_metrics

    called = []
    monkeypatch.setattr(
        coverage_metrics,
        "coverage_metrics_cli",
        lambda: called.append("coverage_metrics_cli"),
    )

    runpy.run_path(
        str(Path(__file__).resolve().parents[1] / "ops" / "coverage_metrics.py"),
        run_name="__main__",
    )

    assert called == ["coverage_metrics_cli"]


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
