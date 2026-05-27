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


def test_load_json_rejects_huge_file(tmp_path: Path) -> None:
    path = tmp_path / "big.json"
    path.write_bytes(b"[]" + b" " * (rt._MAX_JSON_BYTES + 1))
    with pytest.raises(RuntimeError, match="too large"):
        rt._load_json(path)


def test_llvm_cov_tries_nextest_last(monkeypatch) -> None:
    from ops import cd_runtime_llvm as llvm

    calls: list[list[str]] = []

    def _fake_check(cmd: list[str], *, cwd: Path | None = None) -> int:
        _ = cwd
        calls.append(cmd)
        return 1

    monkeypatch.setattr(llvm, "_run_check_only", _fake_check)
    with pytest.raises(RuntimeError, match="llvm-cov failed"):
        llvm.llvm_cov_per_file(Path("."))
    assert calls[0] == ["cargo", "llvm-cov", "--lib", "--summary-only"]
    assert calls[-1][2] == "nextest"


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
