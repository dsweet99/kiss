
from __future__ import annotations

import runpy
import sys
from pathlib import Path

import ops.adversarial as ops_adversarial
import ops.coverage_maps as ops_coverage_maps
import ops.coverage_metrics as ops_coverage_metrics

_ROOT = Path(__file__).resolve().parents[1]
_OPS = _ROOT / "ops"


def test_ops_modules_export_main() -> None:
    assert callable(ops_adversarial.main)
    assert callable(ops_coverage_maps.main)
    assert callable(ops_coverage_metrics.main)


def _run_ops_as_main(script: str, monkeypatch, *, patch_target: str, marker: str) -> None:
    called: list[str] = []
    monkeypatch.setattr(patch_target, lambda *a, **k: called.append(marker))
    root = str(_ROOT)
    sys.path[:] = [p for p in sys.path if p != root]
    runpy.run_path(str(_OPS / script), run_name="__main__")
    assert called == [marker]


def test_ops_coverage_metrics_script_runpy(monkeypatch) -> None:
    _run_ops_as_main(
        "coverage_metrics.py",
        monkeypatch,
        patch_target="python.coverage_metrics.coverage_metrics_cli",
        marker="coverage_metrics_cli",
    )


def test_ops_coverage_maps_script_runpy(monkeypatch) -> None:
    _run_ops_as_main(
        "coverage_maps.py",
        monkeypatch,
        patch_target="python.coverage_maps_cli.main",
        marker="coverage_maps",
    )


def test_ops_adversarial_script_runpy(monkeypatch) -> None:
    _run_ops_as_main(
        "adversarial.py",
        monkeypatch,
        patch_target="python.adversarial_cli.main",
        marker="adversarial",
    )


def test_python_coverage_metrics_cli_script_delegates(monkeypatch) -> None:
    test_ops_coverage_metrics_script_runpy(monkeypatch)
