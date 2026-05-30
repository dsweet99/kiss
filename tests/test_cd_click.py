from __future__ import annotations

from pathlib import Path

from ops.cd_click import report_options


def test_report_options_decorator() -> None:
    @report_options
    def cmd(repo: Path, report_out: Path, *, detailed: bool) -> None:
        pass

    assert callable(cmd)
