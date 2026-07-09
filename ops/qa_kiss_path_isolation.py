#!/usr/bin/env python3
"""QA actual coverage indexes from root and nested working directories."""

from __future__ import annotations

import json
import os
import tempfile
from pathlib import Path, PurePosixPath

from qa_kiss_coverage_stress import (
    JOBS,
    KISS,
    PY_SOURCE,
    RS_SOURCE,
    assert_metric,
    changed_text,
    copy_fixture,
    kiss_command,
    language_ignores,
    metric_int,
    rendered_plan,
    run,
)


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


def main() -> None:
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kiss-qa-paths-") as tmp:
        fixture = Path(tmp) / "repo"
        copy_fixture(fixture)
        nested = fixture / "src" / "test_runner"
        assert nested.is_dir(), nested
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
            language: language_ignores(fixture, language)
            for language in ("python", "rust")
        }
        print(
            "fixture:",
            fixture,
            f"python_ignores={len(ignores['python'])}",
            f"rust_ignores={len(ignores['rust'])}",
        )

        cold_metrics: dict[str, dict[str, str]] = {}
        warm_metrics: dict[str, dict[str, str]] = {}
        for language in ("python", "rust"):
            dry_command = kiss_command(
                language,
                ignores[language],
                "--dry-run",
                "--metrics",
                "-j",
                "2",
            )
            root_dry = run(f"{language}-root-dry", dry_command, fixture, env)
            nested_dry = run(f"{language}-nested-dry", dry_command, nested, env)
            assert rendered_plan(root_dry) == rendered_plan(nested_dry), (
                f"{language}: root and nested working directories produced "
                "different plans"
            )
            assert f"{language.upper()} COVERAGE POPULATION" in root_dry.stdout

            cold = run(
                f"{language}-nested-population",
                kiss_command(
                    language,
                    ignores[language],
                    "--metrics",
                    "-j",
                    "2",
                ),
                nested,
                env,
            )
            cold_metrics[language] = cold.metrics()

        py_index = load_json(fixture / ".kiss/rslip_cache/index.json")
        rs_index = load_json(fixture / ".kiss/rust_llvm_cov_cache/index.json")
        py_manifest = load_json(fixture / ".kiss/rslip_cache/population.json")
        rs_manifest = load_json(fixture / ".kiss/rust_llvm_cov_cache/population.json")

        assert Path(py_index["source_root"]).resolve() == fixture.resolve()
        assert Path(rs_index["source_root"]).resolve() == fixture.resolve()
        assert Path(py_manifest["source_root"]).resolve() == fixture.resolve()
        assert Path(rs_manifest["source_root"]).resolve() == fixture.resolve()
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
        assert metric_int(cold_metrics["rust"], "worker_slot_count") <= 2

        for language in ("python", "rust"):
            warm = run(
                f"{language}-nested-warm",
                kiss_command(
                    language,
                    ignores[language],
                    "--metrics",
                    "-j",
                    "2",
                ),
                nested,
                env,
            )
            warm_metrics[language] = warm.metrics()

        py_selected = metric_int(warm_metrics["python"], "python_total")
        rs_selected = metric_int(warm_metrics["rust"], "rust_final_total")
        assert 0 < py_selected < py_population, (py_selected, py_population)
        assert 0 < rs_selected < rs_population, (rs_selected, rs_population)
        assert metric_int(warm_metrics["python"], "python_cache_hits") == py_selected
        assert (
            metric_int(warm_metrics["rust"], "rust_final_cache_hits") == rs_selected
        )

        print(
            "QA PASS: nested-CWD plans match root; persisted indexes are "
            "repo-relative and noise-free; manifests match populations; "
            "worker/artifact bounds and warm selection held."
        )


if __name__ == "__main__":
    main()
