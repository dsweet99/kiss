"""Run external tools to collect runtime line coverage."""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
from pathlib import Path

import click

from python.coverage_stats import normalize_path, repo_has_python, repo_has_rust


def run_slipcover(repo: Path) -> dict[str, float]:
    if shutil.which("slipcover") is None:
        raise click.ClickException("slipcover not found on PATH (needed for Python coverage)")

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "coverage.json"
        cmd = [
            "slipcover",
            "--json",
            "--source",
            ".",
            "--out",
            str(out),
            "-m",
            "pytest",
        ]
        result = subprocess.run(
            cmd,
            cwd=repo,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise click.ClickException(
                "slipcover/pytest failed\n"
                f"command: {' '.join(cmd)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        if not out.exists():
            raise click.ClickException("slipcover did not write coverage JSON")

        payload = json.loads(out.read_text())
        files = payload.get("files", {})
        coverage: dict[str, float] = {}
        for rel_path, info in files.items():
            pct = info.get("summary", {}).get("percent_covered")
            if pct is not None:
                coverage[normalize_path(rel_path, repo)] = float(pct)
        return coverage


def parse_llvm_cov_payload(payload: dict, repo: Path) -> dict[str, float]:
    coverage: dict[str, float] = {}
    for item in payload.get("data", []):
        for file_info in item.get("files", []):
            filename = file_info.get("filename")
            pct = file_info.get("summary", {}).get("lines", {}).get("percent")
            if filename is None or pct is None:
                continue
            coverage[normalize_path(filename, repo)] = float(pct)
    return coverage


def run_llvm_cov(repo: Path) -> dict[str, float]:
    if shutil.which("cargo") is None:
        raise click.ClickException("cargo not found on PATH (needed for Rust coverage)")

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "coverage.json"
        cmd = ["cargo", "llvm-cov", "--json", "--output-path", str(out)]
        result = subprocess.run(
            cmd,
            cwd=repo,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise click.ClickException(
                "cargo llvm-cov failed\n"
                f"command: {' '.join(cmd)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        if not out.exists():
            raise click.ClickException("cargo llvm-cov did not write coverage JSON")

        return parse_llvm_cov_payload(json.loads(out.read_text()), repo)


def run_true_coverage(repo: Path) -> dict[str, float]:
    has_py = repo_has_python(repo)
    has_rs = repo_has_rust(repo)
    if not has_py and not has_rs:
        raise click.ClickException(f"no Python or Rust sources found under {repo}")

    coverage: dict[str, float] = {}
    if has_py:
        coverage.update(run_slipcover(repo))
    if has_rs:
        for path, pct in run_llvm_cov(repo).items():
            coverage[path] = pct
    return coverage
