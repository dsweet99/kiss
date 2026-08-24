
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

from python.coverage_stats import normalize_path, repo_has_python, repo_has_rust


def _read_pytest_testpaths(repo: Path) -> list[str]:
    pyproject = repo / "pyproject.toml"
    if not pyproject.is_file():
        return []
    try:
        data = tomllib.loads(pyproject.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return []
    pytest_ini = data.get("tool", {}).get("pytest", {}).get("ini_options", {})
    testpaths = pytest_ini.get("testpaths")
    if testpaths is None:
        return []
    if isinstance(testpaths, str):
        return [testpaths]
    return [str(p) for p in testpaths]


def _default_pytest_targets(repo: Path) -> list[str]:
    for name in ("tests", "test"):
        if (repo / name).is_dir():
            return [name]
    return []


def pytest_targets(repo: Path) -> list[str]:
    paths = _read_pytest_testpaths(repo)
    return paths if paths else _default_pytest_targets(repo)


def _sklearn_shadows_site_package(repo: Path) -> bool:
    return (repo / "sklearn" / "__init__.py").is_file()


def slipcover_invocation(repo: Path) -> tuple[Path, list[str]]:
    targets = pytest_targets(repo)
    base = ["--continue-on-collection-errors"]
    if _sklearn_shadows_site_package(repo):
        return Path("/tmp"), [*base, "--pyargs", "sklearn.tests"]
    if targets:
        return repo, [*base, *targets]
    return repo, base


def _parse_slipcover_json(out: Path, repo: Path) -> dict[str, float]:
    payload = json.loads(out.read_text())
    files = payload.get("files", {})
    coverage: dict[str, float] = {}
    for rel_path, info in files.items():
        pct = info.get("summary", {}).get("percent_covered")
        if pct is not None:
            coverage[normalize_path(rel_path, repo)] = float(pct)
    return coverage


def run_slipcover(repo: Path) -> dict[str, float]:
    if shutil.which("slipcover") is None:
        raise RuntimeError("slipcover not found on PATH (needed for Python coverage)")

    repo = repo.resolve()
    cwd, pytest_args = slipcover_invocation(repo)
    source = str(repo)

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "coverage.json"
        cmd = [
            "slipcover",
            "--json",
            "--source",
            source,
            "--out",
            str(out),
            "-m",
            "pytest",
            *pytest_args,
        ]
        env = os.environ.copy()
        if cwd == Path("/tmp"):
            env.pop("PYTHONPATH", None)
        result = subprocess.run(
            cmd,
            cwd=cwd,
            capture_output=True,
            text=True,
            env=env,
        )
        if not out.is_file():
            raise RuntimeError(
                "slipcover/pytest failed\n"
                f"command: {' '.join(cmd)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        coverage = _parse_slipcover_json(out, repo)
        if not coverage:
            raise RuntimeError(
                "slipcover/pytest failed\n"
                f"command: {' '.join(cmd)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        if result.returncode != 0:
            print(
                f"slipcover: pytest exit {result.returncode}, "
                f"using partial coverage for {len(coverage)} files",
                file=sys.stderr,
            )
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
        raise RuntimeError("cargo not found on PATH (needed for Rust coverage)")

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "coverage.json"
        cmd = ["cargo", "llvm-cov", "nextest", "--json", "--output-path", str(out)]
        result = subprocess.run(
            cmd,
            cwd=repo,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise RuntimeError(
                "cargo llvm-cov failed\n"
                f"command: {' '.join(cmd)}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        if not out.exists():
            raise RuntimeError("cargo llvm-cov did not write coverage JSON")

        return parse_llvm_cov_payload(json.loads(out.read_text()), repo)


def run_true_coverage(repo: Path) -> dict[str, float]:
    has_py = repo_has_python(repo)
    has_rs = repo_has_rust(repo)
    if not has_py and not has_rs:
        raise RuntimeError(f"no Python or Rust sources found under {repo}")

    coverage: dict[str, float] = {}
    if has_py:
        coverage.update(run_slipcover(repo))
    if has_rs:
        for path, pct in run_llvm_cov(repo).items():
            coverage[path] = pct
    return coverage
