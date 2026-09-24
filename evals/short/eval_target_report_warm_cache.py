"""Warm-cache queries over Uniform Target Reports forms."""

from __future__ import annotations

import os
import subprocess
import tempfile
import time
from pathlib import Path

from evals._harness import KISS, emit_eval, report_eval, run, write_witness_config
from evals.short.eval_timing_kiss_test import write_complex_test_repo

WARM_QUERIES: list[tuple[str, list[str]]] = [
    ("workspace", []),
    ("source-file", ["pkg/app.py"]),
    ("source-dir", ["pkg"]),
    ("test-file", ["tests/test_functions.py"]),
    ("nodeid", ["tests/test_functions.py::test_alpha"]),
    ("symbol", ["pkg/app.py::alpha"]),
    ("union", ["pkg/app.py", "src/lib.rs"]),
    ("rust-source", ["src/lib.rs"]),
    ("nested-dir", ["tests/nested"]),
    ("dry-nodeid", ["--dry-run", "tests/test_functions.py::test_alpha"]),
    ("commit", ["commit"]),
    ("base", ["base"]),
    ("explicit-base", ["base", "--base-branch", "main"]),
    ("main", ["main"]),
    ("explicit-main", ["main", "--main-branch", "main"]),
    ("lang-python", ["--lang", "python"]),
]


def _run(repo: Path, env: dict[str, str], name: str, args: list[str]):
    return run(name, [str(KISS), "test", *args], repo, env, expected=0, timeout=40)


def prepare_git_query_refs(repo: Path) -> None:
    """Give automatic `base` and default `main` distinct valid refs."""
    subprocess.run(["git", "branch", "-M", "main"], cwd=repo, check=True)
    subprocess.run(["git", "checkout", "-b", "feature"], cwd=repo, check=True)
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "feature baseline"],
        cwd=repo,
        check=True,
    )


def warm_cache_queries() -> None:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-target-report-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_complex_test_repo(repo)
        prepare_git_query_refs(repo)
        write_witness_config(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)
        seed_started = time.monotonic()
        _run(repo, env, "target-report-seed", [])
        seed_s = time.monotonic() - seed_started
        warm_started = time.monotonic()
        running = 0
        warm_text = []
        for name, args in WARM_QUERIES:
            outcome = _run(repo, env, f"target-report-{name}", args)
            running += outcome.stdout.count("kiss test: Running")
            warm_text.append(outcome.stdout)
            warm_text.append(outcome.stderr)
        warm_s = time.monotonic() - warm_started
        joined = "".join(warm_text)

        def kernel_sum(field: str) -> int:
            total = 0
            prefix = f"{field}="
            for line in joined.splitlines():
                if "kiss test: kernel " not in line:
                    continue
                for part in line.split():
                    if part.startswith(prefix):
                        total += int(part.split("=", 1)[1])
            return total

        plans = repo / "target/kiss-plan/target-plans"
        reports = repo / "target/kiss-plan/target-reports"
        pointers = reports / "pointers"
        pointer_n = len(list(pointers.glob("*.json"))) if pointers.is_dir() else 0
        emit_eval("target_report_seed_s", "SMALLER", f"{seed_s:.4f}")
        emit_eval("target_report_warm_query_s", "SMALLER", f"{warm_s:.4f}")
        emit_eval("target_report_warm_running_lines", "SMALLER", running)
        assert running == 0, f"warm queries executed tests: {joined}"
        assert kernel_sum("subprocess") == 0, f"warm queries started subprocesses: {joined}"
        emit_eval(
            "target_report_warm_zero_subprocess",
            "LARGER",
            int(kernel_sum("subprocess") == 0 and running == 0),
        )
        emit_eval("target_report_warm_query_n", "LARGER", len(WARM_QUERIES))
        emit_eval("target_report_subprocess_n", "LARGER", 1 + len(WARM_QUERIES))
        emit_eval("target_report_parse_n", "SMALLER", kernel_sum("parse"))
        emit_eval("target_report_git_n", "SMALLER", kernel_sum("git"))
        emit_eval("target_report_index_rebuild_n", "SMALLER", kernel_sum("index"))
        emit_eval("target_report_graph_n", "SMALLER", kernel_sum("graph"))
        emit_eval("target_report_snapshot_retry_n", "SMALLER", kernel_sum("snapshot"))
        emit_eval("target_report_plan_store", "LARGER", int(plans.is_dir()))
        emit_eval("target_report_report_store", "LARGER", int(reports.is_dir()))
        emit_eval("target_report_pointer_files", "LARGER", pointer_n)


def eval_target_report_warm_cache() -> None:
    report_eval(warm_cache_queries)
