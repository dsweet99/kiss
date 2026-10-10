from __future__ import annotations

import os
from pathlib import Path

from evals._harness import run_observed


def test_run_observed_elapsed_stops_when_child_exits(tmp_path: Path) -> None:
    outcome = run_observed(
        "short-child",
        ["python3", "-c", "pass"],
        tmp_path,
        os.environ.copy(),
        sample_interval=0.5,
    )
    assert outcome.returncode == 0
    assert outcome.elapsed < 0.25
