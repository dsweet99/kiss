from __future__ import annotations

from pathlib import Path

import pytest

from ops import cd_runtime as rt


def _patch_kiss_stats_path(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, stats_path: Path
) -> None:
    def _fake_run_to_temp_files(
        cmd: list[str], *, cwd: Path | None = None
    ) -> tuple[int, Path, Path]:
        assert cmd[0] == "kiss"
        return 0, stats_path, tmp_path / "empty.stderr"

    monkeypatch.setattr("ops.cd_runtime_kiss._run_to_temp_files", _fake_run_to_temp_files)


def _patch_kiss_stats_stdout(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, stats_text: str
) -> None:
    stats_path = tmp_path / "stats.txt"
    stats_path.write_text(stats_text)
    _patch_kiss_stats_path(monkeypatch, tmp_path, stats_path)


def test_kiss_summary_median_scans_prefix_without_full_read(
    monkeypatch, tmp_path: Path
) -> None:
    _patch_kiss_stats_stdout(
        monkeypatch,
        tmp_path,
        "inv_test_coverage p50 42\n" + ("noise\n" * 1000),
    )
    assert rt.kiss_summary_median(tmp_path) == 58.0


def test_kiss_summary_median_finds_inv_line_after_large_prefix(
    monkeypatch, tmp_path: Path
) -> None:
    stats_path = tmp_path / "stats.txt"
    with stats_path.open("w", encoding="utf-8") as handle:
        for _ in range(600_000):
            handle.write("y\n")
        handle.write("inv_test_coverage p50 42\n")
    _patch_kiss_stats_path(monkeypatch, tmp_path, stats_path)
    assert rt.kiss_summary_median(tmp_path) == 58.0
