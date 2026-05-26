from __future__ import annotations

import gc
import json
import resource
from pathlib import Path

import pytest

from ops import cd_runtime as rt


def _rss_mb() -> float:
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / (1024 * 1024)


def test_run_small_output() -> None:
    assert rt.run(["echo", "ok"]) == "ok\n"


def test_run_rejects_huge_stdout() -> None:
    n = 2 * rt._MAX_RUN_BYTES
    with pytest.raises(RuntimeError, match="too large"):
        rt.run(["python3", "-c", f"print('x' * {n})"])


def test_success_path_unlink_does_not_load_huge_stderr() -> None:
    gc.collect()
    before = _rss_mb()
    code, stdout_path, stderr_path = rt._run_to_temp_files(
        [
            "python3",
            "-c",
            "import sys; sys.stderr.write('z' * (50 * 1024 * 1024)); print('done')",
        ]
    )
    rt._unlink_paths(stdout_path, stderr_path)
    gc.collect()
    after = _rss_mb()
    assert code == 0
    assert after - before < 5.0


def test_kiss_summary_median_scans_prefix_without_full_read(
    monkeypatch, tmp_path: Path
) -> None:
    stats_path = tmp_path / "stats.txt"
    stats_path.write_text("inv_test_coverage p50 42\n" + ("noise\n" * 1000))

    def _fake_run_to_temp_files(
        cmd: list[str], *, cwd: Path | None = None
    ) -> tuple[int, Path, Path]:
        assert cmd[0] == "kiss"
        return 0, stats_path, tmp_path / "empty.stderr"

    monkeypatch.setattr("ops.cd_runtime_kiss._run_to_temp_files", _fake_run_to_temp_files)
    assert rt.kiss_summary_median(tmp_path) == 58.0


def test_slipcover_success_unlinks_streams_without_reading(
    monkeypatch, tmp_path: Path
) -> None:
    repo = tmp_path / "proj"
    repo.mkdir()
    out_path = tmp_path / "slipcover.json"
    payload = {
        "files": {
            "m.py": {"summary": {"covered_lines": 1, "missing_lines": 0}},
        },
        "summary": {"percent_covered": 100.0},
    }
    out_path.write_text(json.dumps(payload))

    def _fake_slipcover(spec: rt._SlipcoverRun) -> None:
        Path(spec.out_path).write_text(out_path.read_text())

    monkeypatch.setattr("ops.cd_runtime_slipcover._run_slipcover", _fake_slipcover)
    per_file, total = rt.slipcover_per_file(repo, ["tests/"])
    assert total == 100.0
    assert (repo / "m.py").resolve() in per_file
