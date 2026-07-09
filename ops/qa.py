#!/usr/bin/env python3
"""Long-running integration QA commands for the local development `kiss`."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Iterator

import click

ROOT = Path(__file__).resolve().parents[1]
KISS = ROOT / "target" / "debug" / "kiss"
PY_SOURCE = Path("python/coverage_metrics.py")
PY_TEST = Path("tests/test_coverage_metrics_kiss.py")
RS_SOURCE = Path("src/cli_output.rs")
LANGUAGES = ("python", "rust")


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
        result: dict[str, str] = {}
        for line in self.stdout.splitlines():
            key, separator, value = line.partition("=")
            if separator and key and " " not in key:
                result[key] = value
        return result


@dataclass
class Fixture:
    root: Path
    nested: Path
    env: dict[str, str]
    ignores: dict[str, list[str]]


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
    click.echo(f"{name}: rc={outcome.returncode} elapsed={outcome.elapsed:.2f}s")
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
        click.echo(f"  {summary}")
    if expected is not None and outcome.returncode != expected:
        raise AssertionError(
            f"{name}: expected rc={expected}, got {outcome.returncode}\n"
            f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
        )
    return outcome


def run_concurrent(
    name: str,
    commands: list[tuple[list[str], Path]],
    env: dict[str, str],
    timeout: int = 1_200,
) -> list[Outcome]:
    started = time.monotonic()
    processes = [
        subprocess.Popen(
            argv,
            cwd=cwd,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for argv, cwd in commands
    ]
    outcomes: list[Outcome] = []
    for index, process in enumerate(processes):
        stdout, stderr = process.communicate(timeout=timeout)
        outcomes.append(
            Outcome(
                f"{name}-{index}",
                process.returncode,
                stdout,
                stderr,
                time.monotonic() - started,
            )
        )
    click.echo(f"{name}: {len(outcomes)} processes, elapsed={time.monotonic() - started:.2f}s")
    for outcome in outcomes:
        click.echo(f"  {outcome.name}: rc={outcome.returncode}")
        if outcome.returncode != 0:
            click.echo(outcome.stdout)
            click.echo(outcome.stderr, err=True)
    failed = [outcome for outcome in outcomes if outcome.returncode != 0]
    assert not failed, f"{name}: {len(failed)} concurrent process(es) failed"
    return outcomes


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


def language_ignores(root: Path, language: str) -> list[str]:
    if language == "python":
        ignored = [
            path.name
            for path in root.rglob("*.py")
            if test_file(path.relative_to(root)) and path.relative_to(root) != PY_TEST
        ]
    else:
        ignored = [
            path.name for path in root.rglob("*.rs") if path.relative_to(root) != RS_SOURCE
        ]
    result: list[str] = []
    for path in sorted(set(ignored)):
        result.extend(["--ignore", path])
    return result


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


def validate_command(language: str, ignores: list[str], jobs: int) -> list[str]:
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
        str(jobs),
        *ignores,
    ]


@contextmanager
def qa_fixture(prefix: str) -> Iterator[Fixture]:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix=prefix) as tmp:
        root = Path(tmp) / "repo"
        copy_fixture(root)
        nested = root / "src" / "test_runner"
        assert nested.is_dir(), nested
        env = os.environ.copy()
        env["PYTHONPATH"] = str(root)
        env.pop("RUSTFLAGS", None)
        changed_text(
            root / PY_SOURCE,
            "return {path: partial.get(path, 0.0) for path in files}",
            "return {path: partial.get(path, float(0)) for path in files}",
        )
        changed_text(root / RS_SOURCE, "if file_pct >= 100 {", "if 100 <= file_pct {")
        ignores = {language: language_ignores(root, language) for language in LANGUAGES}
        click.echo(
            f"fixture: {root} python_ignores={len(ignores['python'])} "
            f"rust_ignores={len(ignores['rust'])}"
        )
        yield Fixture(root, nested, env, ignores)


def load_json(path: Path) -> dict:
    assert path.is_file(), f"missing persisted artifact: {path}"
    return json.loads(path.read_text())


def assert_repo_relative_index(index: dict, expected_source: str) -> None:
    source_root = Path(index["source_root"])
    assert source_root.is_absolute(), source_root
    files = index["files"]
    assert expected_source in files, (
        f"changed source {expected_source!r} absent from index keys: "
        f"{sorted(files)[:20]}"
    )
    assert files, "coverage index unexpectedly empty"
    for file in files:
        pure = PurePosixPath(file)
        assert not pure.is_absolute(), file
        assert ".." not in pure.parts, file
        assert not file.startswith(".kiss/"), file
        assert not file.startswith("<"), file
        assert "rslip_runtime.py" not in file, file


def assert_json_integrity(cache_root: Path) -> int:
    json_paths = sorted(cache_root.rglob("*.json"))
    assert json_paths, f"no JSON artifacts under {cache_root}"
    for path in json_paths:
        try:
            json.loads(path.read_text())
        except Exception as error:
            raise AssertionError(f"invalid JSON artifact {path}: {error}") from error
    temporary = sorted(cache_root.rglob("*.tmp"))
    assert not temporary, f"temporary files survived: {temporary}"
    return len(json_paths)


@click.group()
def cli() -> None:
    """Run long, disposable QA scenarios against target/debug/kiss."""


@cli.command("coverage-stress")
def coverage_stress() -> None:
    """Stress population, selection, force, env invalidation, and recall."""
    jobs = 4
    with qa_fixture("kiss-qa-stress-") as fixture:
        cold: dict[str, Outcome] = {}
        warm: dict[str, Outcome] = {}
        forced: dict[str, Outcome] = {}

        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            first = run(f"{language}-cold-dry-1", command, fixture.root, fixture.env)
            second = run(f"{language}-cold-dry-2", command, fixture.root, fixture.env)
            assert rendered_plan(first) == rendered_plan(second)
            assert f"{language.upper()} COVERAGE POPULATION" in first.stdout
            cold[language] = run(
                f"{language}-cold-population",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )

        py_cold = cold["python"].metrics()
        rs_cold = cold["rust"].metrics()
        assert_metric(py_cold, "python_population_required", "true")
        assert_metric(rs_cold, "rust_population_required", "true")
        py_population = metric_int(py_cold, "python_population_selectors")
        rs_population = metric_int(rs_cold, "rust_population_selectors")
        assert py_population >= 4
        assert rs_population >= 8
        assert_metric(rs_cold, "raw_artifact_count", "0")
        assert metric_int(rs_cold, "worker_slot_count") <= jobs

        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            first = run(f"{language}-warm-dry-1", command, fixture.root, fixture.env)
            second = run(f"{language}-warm-dry-2", command, fixture.root, fixture.env)
            assert rendered_plan(first) == rendered_plan(second)
            assert "COVERAGE POPULATION" not in first.stdout
            warm[language] = run(
                f"{language}-warm-selective",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )
            forced[language] = run(
                f"{language}-forced-selective",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "--force",
                    "-j",
                    str(jobs),
                ),
                fixture.root,
                fixture.env,
            )
            post_force = run(
                f"{language}-post-force-dry",
                command,
                fixture.root,
                fixture.env,
            )
            assert "COVERAGE POPULATION" not in post_force.stdout, (
                f"{language}: a forced selective run invalidated the warm "
                "population manifest"
            )

        py_selected = metric_int(warm["python"].metrics(), "python_total")
        rs_selected = metric_int(warm["rust"].metrics(), "rust_final_total")
        assert 0 < py_selected < py_population
        assert 0 < rs_selected < rs_population
        assert metric_int(warm["python"].metrics(), "python_cache_hits") == py_selected
        assert metric_int(warm["rust"].metrics(), "rust_final_cache_hits") == rs_selected
        assert metric_int(forced["python"].metrics(), "python_cache_misses") == py_selected
        assert (
            metric_int(forced["rust"].metrics(), "rust_final_cache_misses") == rs_selected
        )

        changed_py_env = fixture.env.copy()
        changed_py_env["PYTHONPATH"] = (
            f"{fixture.root}{os.pathsep}/tmp/kiss-qa-env-change"
        )
        py_env = run(
            "python-env-invalidation",
            kiss_command(
                "python",
                fixture.ignores["python"],
                "--dry-run",
                "-j",
                str(jobs),
            ),
            fixture.root,
            changed_py_env,
        )
        assert "PYTHON COVERAGE POPULATION" in py_env.stdout

        changed_rs_env = fixture.env.copy()
        changed_rs_env["RUSTFLAGS"] = "-Cdebuginfo=0"
        rs_env = run(
            "rust-env-invalidation",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--dry-run",
                "-j",
                str(jobs),
            ),
            fixture.root,
            changed_rs_env,
        )
        assert "RUST COVERAGE POPULATION" in rs_env.stdout

        for language in LANGUAGES:
            validation = run(
                f"{language}-validation",
                validate_command(language, fixture.ignores[language], jobs),
                fixture.root,
                fixture.env,
            )
            metrics = validation.metrics()
            assert 0 < metric_int(metrics, "selected_total") < metric_int(
                metrics, "full_total"
            )

        changed_text(
            fixture.root / PY_SOURCE,
            "return {path: partial.get(path, float(0)) for path in files}",
            "return {path: partial.get(path, 999.0) for path in files}",
        )
        py_regression = run(
            "python-regression-kiss",
            kiss_command(
                "python",
                fixture.ignores["python"],
                "--metrics",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert py_regression.returncode != 0
        py_oracle = run(
            "python-regression-oracle",
            ["python", "-m", "pytest", str(PY_TEST), "-q"],
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert py_oracle.returncode != 0

        changed_text(
            fixture.root / RS_SOURCE,
            "if 100 <= file_pct {",
            "if 1000 <= file_pct {",
        )
        rs_regression = run(
            "rust-regression-kiss",
            kiss_command(
                "rust",
                fixture.ignores["rust"],
                "--metrics",
                "-j",
                str(jobs),
            ),
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert rs_regression.returncode != 0
        rs_oracle = run(
            "rust-regression-oracle",
            [
                "cargo",
                "test",
                "test_format_unreferenced_unit_coverage_message_rounding_cliff",
            ],
            fixture.root,
            fixture.env,
            expected=None,
        )
        assert rs_oracle.returncode != 0
        click.echo("QA PASS: coverage lifecycle and oracle recall held.")


@cli.command("path-isolation")
def path_isolation() -> None:
    """Test nested-CWD plans and persisted coverage-path isolation."""
    jobs = 2
    with qa_fixture("kiss-qa-paths-") as fixture:
        cold_metrics: dict[str, dict[str, str]] = {}
        warm_metrics: dict[str, dict[str, str]] = {}
        for language in LANGUAGES:
            dry_command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            root_dry = run(
                f"{language}-root-dry", dry_command, fixture.root, fixture.env
            )
            nested_dry = run(
                f"{language}-nested-dry", dry_command, fixture.nested, fixture.env
            )
            assert rendered_plan(root_dry) == rendered_plan(nested_dry)
            assert f"{language.upper()} COVERAGE POPULATION" in root_dry.stdout
            cold = run(
                f"{language}-nested-population",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            cold_metrics[language] = cold.metrics()

        py_cache = fixture.root / ".kiss/rslip_cache"
        rs_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        py_index = load_json(py_cache / "index.json")
        rs_index = load_json(rs_cache / "index.json")
        py_manifest = load_json(py_cache / "population.json")
        rs_manifest = load_json(rs_cache / "population.json")
        for payload in (py_index, rs_index, py_manifest, rs_manifest):
            assert Path(payload["source_root"]).resolve() == fixture.root.resolve()
        assert_repo_relative_index(py_index, PY_SOURCE.as_posix())
        assert_repo_relative_index(rs_index, RS_SOURCE.as_posix())

        py_population = metric_int(
            cold_metrics["python"], "python_population_selectors"
        )
        rs_population = metric_int(cold_metrics["rust"], "rust_population_selectors")
        assert len(py_manifest["selectors"]) == py_population
        assert len(rs_manifest["selectors"]) == rs_population
        assert len(set(py_manifest["selectors"])) == py_population
        assert len(set(rs_manifest["selectors"])) == rs_population
        assert_metric(cold_metrics["rust"], "raw_artifact_count", "0")
        assert metric_int(cold_metrics["rust"], "worker_slot_count") <= jobs

        for language in LANGUAGES:
            warm = run(
                f"{language}-nested-warm",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            warm_metrics[language] = warm.metrics()
        py_selected = metric_int(warm_metrics["python"], "python_total")
        rs_selected = metric_int(warm_metrics["rust"], "rust_final_total")
        assert 0 < py_selected < py_population
        assert 0 < rs_selected < rs_population
        assert metric_int(warm_metrics["python"], "python_cache_hits") == py_selected
        assert (
            metric_int(warm_metrics["rust"], "rust_final_cache_hits") == rs_selected
        )
        click.echo(
            "QA PASS: CWD-stable plans, clean persisted paths, manifests, "
            "worker bounds, and warm selection held."
        )


@cli.command("concurrent-cache-recovery")
def concurrent_cache_recovery() -> None:
    """Race shared caches, then test malformed-index fail-safe recovery."""
    jobs = 2
    with qa_fixture("kiss-qa-concurrent-") as fixture:
        cold_commands: list[tuple[list[str], Path]] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--metrics",
                "-j",
                str(jobs),
            )
            cold_commands.extend(
                [
                    (command, fixture.root),
                    (command, fixture.nested),
                    (command, fixture.root),
                ]
            )
        cold = run_concurrent("cold-race", cold_commands, fixture.env)
        for outcome in cold[:3]:
            assert outcome.metrics()["python_population_required"] == "true"
        for outcome in cold[3:]:
            assert outcome.metrics()["rust_population_required"] == "true"

        py_cache = fixture.root / ".kiss/rslip_cache"
        rs_cache = fixture.root / ".kiss/rust_llvm_cov_cache"
        py_json_count = assert_json_integrity(py_cache)
        rs_json_count = assert_json_integrity(rs_cache)
        assert not list((rs_cache / "artifacts").glob("*"))
        workers = [
            path
            for path in (rs_cache / "workers").glob("slot-*")
            if path.is_dir()
        ]
        assert len(workers) <= jobs
        click.echo(
            f"cold integrity: python_json={py_json_count} "
            f"rust_json={rs_json_count} workers={len(workers)}"
        )

        dry_commands: list[tuple[list[str], Path]] = []
        dry_languages: list[str] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                str(jobs),
            )
            for cwd in (
                fixture.root,
                fixture.nested,
                fixture.root,
                fixture.nested,
            ):
                dry_commands.append((command, cwd))
                dry_languages.append(language)
        dry = run_concurrent("warm-dry-race", dry_commands, fixture.env)
        for language in LANGUAGES:
            plans = [
                rendered_plan(outcome)
                for outcome, outcome_language in zip(
                    dry, dry_languages, strict=True
                )
                if outcome_language == language
            ]
            assert len(set(plans)) == 1
            assert "COVERAGE POPULATION" not in plans[0]

        warm_commands: list[tuple[list[str], Path]] = []
        warm_languages: list[str] = []
        for language in LANGUAGES:
            command = kiss_command(
                language,
                fixture.ignores[language],
                "--metrics",
                "-j",
                str(jobs),
            )
            for cwd in (fixture.root, fixture.nested):
                warm_commands.append((command, cwd))
                warm_languages.append(language)
        warm = run_concurrent("warm-execution-race", warm_commands, fixture.env)
        for outcome, language in zip(warm, warm_languages, strict=True):
            metrics = outcome.metrics()
            if language == "python":
                total = metric_int(metrics, "python_total")
                hits = metric_int(metrics, "python_cache_hits")
            else:
                total = metric_int(metrics, "rust_final_total")
                hits = metric_int(metrics, "rust_final_cache_hits")
            assert total > 0
            assert hits == total

        for language, index_path in (
            ("python", py_cache / "index.json"),
            ("rust", rs_cache / "index.json"),
        ):
            index_path.write_text("{ deliberately broken")
            corrupted_dry = run(
                f"{language}-corrupt-index-dry",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--dry-run",
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            assert f"{language.upper()} COVERAGE POPULATION" in corrupted_dry.stdout
            repaired = run(
                f"{language}-corrupt-index-repair",
                kiss_command(
                    language,
                    fixture.ignores[language],
                    "--metrics",
                    "-j",
                    str(jobs),
                ),
                fixture.nested,
                fixture.env,
            )
            assert repaired.metrics()[f"{language}_population_required"] == "true"
            json.loads(index_path.read_text())

        assert_json_integrity(py_cache)
        assert_json_integrity(rs_cache)
        assert not list((rs_cache / "artifacts").glob("*"))
        click.echo(
            "QA PASS: concurrent cold/warm races and malformed-index recovery held."
        )


if __name__ == "__main__":
    cli()
