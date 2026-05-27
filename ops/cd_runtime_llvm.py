from __future__ import annotations

import tempfile
from pathlib import Path

from ops.cd_runtime_io import _load_json, _run_check_only, _run_to_temp_files, _unlink_paths


def _llvm_cov_from_data(data: object) -> tuple[dict[Path, float], float]:
    assert isinstance(data, dict)
    per_file: dict[Path, float] = {}
    total_lines = covered_lines = 0
    for entry in data.get("data", []):
        assert isinstance(entry, dict)
        for f in entry.get("files", []):
            assert isinstance(f, dict)
            path = Path(str(f["filename"])).resolve()
            lines = f.get("summary", {}).get("lines", {})
            assert isinstance(lines, dict)
            count = int(lines.get("count", 0))
            covered = int(lines.get("covered", 0))
            pct = float(lines.get("percent", 0.0))
            if count > 0:
                per_file[path] = pct
                total_lines += count
                covered_lines += covered
    total_pct = (100.0 * covered_lines / total_lines) if total_lines else 0.0
    return per_file, total_pct


def _run_llvm_report_cmd(repo: Path, report_path: Path) -> None:
    code, stdout_path, stderr_path = _run_to_temp_files(
        [
            "cargo",
            "llvm-cov",
            "report",
            "--json",
            "--summary-only",
            "--output-path",
            str(report_path),
        ],
        cwd=repo,
    )
    _unlink_paths(stdout_path, stderr_path)
    if code != 0 or report_path.stat().st_size == 0:
        raise RuntimeError(f"cargo llvm-cov report failed in {repo}")


_LLVM_COV_TRY_CMDS: tuple[list[str], ...] = (
    ["cargo", "llvm-cov", "--lib", "--summary-only"],
    ["cargo", "llvm-cov", "--summary-only"],
    ["cargo", "llvm-cov", "nextest", "--lib", "--summary-only"],
)


def llvm_cov_per_file(repo: Path) -> tuple[dict[Path, float], float]:
    for cmd in _LLVM_COV_TRY_CMDS:
        if _run_check_only(cmd, cwd=repo) == 0:
            break
    else:
        raise RuntimeError(f"cargo llvm-cov failed in {repo}")

    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as tmp:
        report_path = Path(tmp.name)
    try:
        _run_llvm_report_cmd(repo, report_path)
        return _llvm_cov_from_data(_load_json(report_path))
    finally:
        report_path.unlink(missing_ok=True)
