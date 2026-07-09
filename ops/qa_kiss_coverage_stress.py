#!/usr/bin/env python3
"""Disposable mixed-language stress QA for local `kiss test`.

This intentionally does not install kiss or invoke git directly. It copies the
current repository (including its existing Git metadata) into a temporary
worktree and invokes the locally built development binary there.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
KISS = ROOT / "target" / "debug" / "kiss"
JOBS = 4
PY_SOURCE = Path("python/coverage_metrics.py")
PY_TEST = Path("tests/test_coverage_metrics_kiss.py")
RS_SOURCE = Path("src/cli_output.rs")


@dataclass
class Outcome:
    name: str
    returncode: int
    stdout: str
    stderr: str
    elapsed: float

    @property
    def combined(self) -> str:
        return self.stdout + self.stderr

    def metrics(self) -> dict[str, str]:
        out: dict[str, str] = {}
        for line in self.stdout.splitlines():
            key, separator, value = line.partition("=")
            if separator and key and " " not in key:
                out[key] = value
        return out


def run(
    name: str,
    argv: list[str],
    cwd: Path,
    env: dict[str, str],
    expected: int | None = 0,
    timeout: int = 1_200,
) -> Outcome:
    started = time.monotonic()
    completed = subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    outcome = Outcome(
        name,
        completed.returncode,
        completed.stdout,
        completed.stderr,
        time.monotonic() - started,
    )
    print(f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s")
    metrics = outcome.metrics()
    interesting = (
        "selected_python",
        "selected_rust_initial",
        "python_population_required",
        "rust_population_required",
        "python_population_selectors",
        "rust_population_selectors",
        "python_total",
        "python_cache_hits",
        "python_cache_misses",
        "rust_population_total",
        "rust_population_cache_hits",
        "rust_population_cache_misses",
        "rust_final_total",
        "rust_final_cache_hits",
        "rust_final_cache_misses",
        "raw_artifact_count",
        "worker_slot_count",
    )
    summary = ", ".join(f"{key}={metrics[key]}" for key in interesting if key in metrics)
    if summary:
        print(f"  {summary}")
    if expected is not None and outcome.returncode != expected:
        raise AssertionError(
            f"{name}: expected rc={expected}, got {outcome.returncode}\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    return outcome


def assert_metric(metrics: dict[str, str], key: str, expected: str) -> None:
    actual = metrics.get(key)
    assert actual == expected, f"{key}: expected {expected!r}, got {actual!r}"


def metric_int(metrics: dict[str, str], key: str) -> int:
    assert key in metrics, f"missing metric {key}: {metrics}"
    return int(metrics[key])


def rendered_plan(outcome: Outcome) -> str:
    return outcome.stdout.partition("KISS TEST METRICS")[0]


def changed_text(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    count = text.count(old)
    assert count == 1, f"{path}: expected one occurrence of {old!r}, found {count}"
    path.write_text(text.replace(old, new))


def copy_fixture(destination: Path) -> None:
    ignored = shutil.ignore_patterns(
        "target",
        ".kiss",
        "_kpop",
        ".cursor",
        ".cursorrules",
        ".testmondata",
        ".malvin",
        ".malvin_home",
        "__pycache__",
        ".pytest_cache",
        "log",
        "log_1",
        "o",
    )
    shutil.copytree(ROOT, destination, dirs_exist_ok=True, ignore=ignored)


def test_file(path: Path) -> bool:
    return (
        path.name.startswith("test_")
        or path.name.endswith("_test.py")
        or "tests" in path.parts
        or "test" in path.parts
    )


def language_ignores(fixture: Path, language: str) -> list[str]:
    if language == "python":
        ignored = [
            path.name
            for path in fixture.rglob("*.py")
            if test_file(path.relative_to(fixture))
            and path.relative_to(fixture) != PY_TEST
        ]
    else:
        ignored = [
            path.name
            for path in fixture.rglob("*.rs")
            if path.relative_to(fixture) != RS_SOURCE
        ]
    args: list[str] = []
    for path in sorted(set(ignored)):
        args.extend(["--ignore", path])
    return args


def kiss_command(language: str, ignores: list[str], *options: str) -> list[str]:
    return [
        str(KISS),
        "--defaults",
        "--lang",
        language,
        "test",
        "commit",
        *options,
        *ignores,
    ]


def validate_command(language: str, ignores: list[str]) -> list[str]:
    return [
        str(KISS),
        "--defaults",
        "--lang",
        language,
        "test",
        "validate-selection",
        "commit",
        "--dry-run",
        "-j",
        str(JOBS),
        *ignores,
    ]


def main() -> None:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kiss-qa-stress-") as tmp:
        fixture = Path(tmp) / "repo"
        copy_fixture(fixture)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(fixture)
        env.pop("RUSTFLAGS", None)

        changed_text(
            fixture / PY_SOURCE,
            "return {path: partial.get(path, 0.0) for path in files}",
            "return {path: partial.get(path, float(0)) for path in files}",
        )
        changed_text(
            fixture / RS_SOURCE,
            "if file_pct >= 100 {",
            "if 100 <= file_pct {",
        )

        ignores = {
            "python": language_ignores(fixture, "python"),
            "rust": language_ignores(fixture, "rust"),
        }
        print(
            "fixture:",
            fixture,
            f"python_ignores={len(ignores['python'])}",
            f"rust_ignores={len(ignores['rust'])}",
        )

        cold_dry: dict[str, Outcome] = {}
        cold: dict[str, Outcome] = {}
        warm_dry: dict[str, Outcome] = {}
        warm: dict[str, Outcome] = {}
        forced: dict[str, Outcome] = {}

        for language in ("python", "rust"):
            command = kiss_command(
                language,
                ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(JOBS),
            )
            first = run(f"{language}-cold-dry-1", command, fixture, env)
            second = run(f"{language}-cold-dry-2", command, fixture, env)
            assert rendered_plan(first) == rendered_plan(
                second
            ), f"{language}: cold dry-run was unstable"
            assert f"{language.upper()} COVERAGE POPULATION" in first.stdout
            cold_dry[language] = first

            cold[language] = run(
                f"{language}-cold-population",
                kiss_command(
                    language,
                    ignores[language],
                    "--metrics",
                    "-j",
                    str(JOBS),
                ),
                fixture,
                env,
            )

        py_cold = cold["python"].metrics()
        rs_cold = cold["rust"].metrics()
        assert_metric(py_cold, "python_population_required", "true")
        assert_metric(rs_cold, "rust_population_required", "true")
        py_population = metric_int(py_cold, "python_population_selectors")
        rs_population = metric_int(rs_cold, "rust_population_selectors")
        assert py_population >= 4, py_cold
        assert rs_population >= 8, rs_cold
        assert_metric(rs_cold, "raw_artifact_count", "0")
        assert metric_int(rs_cold, "worker_slot_count") <= JOBS

        for language in ("python", "rust"):
            command = kiss_command(
                language,
                ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(JOBS),
            )
            first = run(f"{language}-warm-dry-1", command, fixture, env)
            second = run(f"{language}-warm-dry-2", command, fixture, env)
            assert rendered_plan(first) == rendered_plan(
                second
            ), f"{language}: warm dry-run was unstable"
            assert "COVERAGE POPULATION" not in first.stdout
            warm_dry[language] = first
            warm[language] = run(
                f"{language}-warm-selective",
                kiss_command(
                    language,
                    ignores[language],
                    "--metrics",
                    "-j",
                    str(JOBS),
                ),
                fixture,
                env,
            )
            forced[language] = run(
                f"{language}-forced-selective",
                kiss_command(
                    language,
                    ignores[language],
                    "--metrics",
                    "--force",
                    "-j",
                    str(JOBS),
                ),
                fixture,
                env,
            )
            post_force = run(
                f"{language}-post-force-dry",
                kiss_command(
                    language,
                    ignores[language],
                    "--dry-run",
                    "--metrics",
                    "-j",
                    str(JOBS),
                ),
                fixture,
                env,
            )
            assert "COVERAGE POPULATION" not in post_force.stdout, (
                f"{language}: a forced selective run invalidated the warm "
                "population manifest"
            )

        py_warm = warm["python"].metrics()
        rs_warm = warm["rust"].metrics()
        assert_metric(py_warm, "python_population_required", "false")
        assert_metric(rs_warm, "rust_population_required", "false")
        py_selected = metric_int(py_warm, "python_total")
        rs_selected = metric_int(rs_warm, "rust_final_total")
        assert 0 < py_selected < py_population, (py_selected, py_population)
        assert 0 < rs_selected < rs_population, (rs_selected, rs_population)
        assert metric_int(py_warm, "python_cache_hits") == py_selected
        assert metric_int(rs_warm, "rust_final_cache_hits") == rs_selected

        py_forced = forced["python"].metrics()
        rs_forced = forced["rust"].metrics()
        assert metric_int(py_forced, "python_cache_misses") == py_selected
        assert metric_int(rs_forced, "rust_final_cache_misses") == rs_selected

        changed_py_env = env.copy()
        changed_py_env["PYTHONPATH"] = f"{fixture}{os.pathsep}/tmp/kiss-qa-env-change"
        py_env = run(
            "python-env-invalidation",
            kiss_command(
                "python",
                ignores["python"],
                "--dry-run",
                "-j",
                str(JOBS),
            ),
            fixture,
            changed_py_env,
        )
        assert "PYTHON COVERAGE POPULATION" in py_env.stdout

        changed_rs_env = env.copy()
        changed_rs_env["RUSTFLAGS"] = "-Cdebuginfo=0"
        rs_env = run(
            "rust-env-invalidation",
            kiss_command(
                "rust",
                ignores["rust"],
                "--dry-run",
                "-j",
                str(JOBS),
            ),
            fixture,
            changed_rs_env,
        )
        assert "RUST COVERAGE POPULATION" in rs_env.stdout

        for language in ("python", "rust"):
            validation = run(
                f"{language}-validation",
                validate_command(language, ignores[language]),
                fixture,
                env,
            )
            vm = validation.metrics()
            selected = metric_int(vm, "selected_total")
            full = metric_int(vm, "full_total")
            assert 0 < selected < full, (language, selected, full)

        changed_text(
            fixture / PY_SOURCE,
            "return {path: partial.get(path, float(0)) for path in files}",
            "return {path: partial.get(path, 999.0) for path in files}",
        )
        py_regression = run(
            "python-regression-kiss",
            kiss_command(
                "python",
                ignores["python"],
                "--metrics",
                "-j",
                str(JOBS),
            ),
            fixture,
            env,
            expected=None,
        )
        assert py_regression.returncode != 0, py_regression.combined
        assert_metric(py_regression.metrics(), "python_population_required", "true")
        py_oracle = run(
            "python-regression-oracle",
            ["python", "-m", "pytest", str(PY_TEST), "-q"],
            fixture,
            env,
            expected=None,
        )
        assert py_oracle.returncode != 0, py_oracle.combined

        changed_text(
            fixture / RS_SOURCE,
            "if 100 <= file_pct {",
            "if 1000 <= file_pct {",
        )
        rs_regression = run(
            "rust-regression-kiss",
            kiss_command(
                "rust",
                ignores["rust"],
                "--metrics",
                "-j",
                str(JOBS),
            ),
            fixture,
            env,
            expected=None,
        )
        assert rs_regression.returncode != 0, rs_regression.combined
        assert_metric(rs_regression.metrics(), "rust_population_required", "true")
        rs_oracle = run(
            "rust-regression-oracle",
            [
                "cargo",
                "test",
                "test_format_unreferenced_unit_coverage_message_rounding_cliff",
            ],
            fixture,
            env,
            expected=None,
        )
        assert rs_oracle.returncode != 0, rs_oracle.combined

        print("QA PASS: cold population, warm selective cache reuse, force rerun,")
        print("environment invalidation, validation, disk bounds, and oracle recall held.")


if __name__ == "__main__":
    main()
