"""Time kiss test on a mixed-language tmp repo with every allowed test style."""

from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path

from evals._harness import (
    KISS,
    commit_fixture_baseline,
    emit_eval,
    report_eval,
    run,
    write_witness_config,
)


def write_complex_test_repo(repo: Path) -> None:
    """Python + Rust tests in every style kiss collects by default."""
    (repo / "pkg").mkdir(parents=True)
    (repo / "src").mkdir(parents=True)
    (repo / "tests" / "nested").mkdir(parents=True)
    write_witness_config(repo)
    (repo / "Cargo.toml").write_text(
        "[package]\n"
        "name = \"kiss_timing_complex\"\n"
        "version = \"0.1.0\"\n"
        "edition = \"2024\"\n",
    )
    (repo / "src/lib.rs").write_text(
        "pub mod math;\n"
        "pub fn alpha() -> &'static str { \"alpha\" }\n"
        "\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[test]\n"
        "    fn unit_alpha() {\n"
        "        assert_eq!(super::alpha(), \"alpha\");\n"
        "    }\n"
        "}\n",
    )
    (repo / "src/math.rs").write_text(
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n"
        "\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    #[test]\n"
        "    fn unit_add() {\n"
        "        assert_eq!(super::add(1, 2), 3);\n"
        "    }\n"
        "}\n",
    )
    (repo / "pkg/__init__.py").write_text("")
    (repo / "pkg/app.py").write_text(
        "def alpha():\n"
        "    return 'alpha'\n"
        "\n"
        "def add(a, b):\n"
        "    return a + b\n",
    )
    (repo / "tests/conftest.py").write_text(
        "import pytest\n"
        "\n"
        "\n"
        "@pytest.fixture\n"
        "def one():\n"
        "    return 1\n",
    )
    (repo / "tests/test_functions.py").write_text(
        "from pkg.app import alpha\n"
        "\n"
        "\n"
        "def test_alpha():\n"
        "    assert alpha() == 'alpha'\n",
    )
    (repo / "tests/classes_test.py").write_text(
        "from pkg.app import add\n"
        "\n"
        "\n"
        "class TestAdd:\n"
        "    def test_sum(self):\n"
        "        assert add(1, 2) == 3\n",
    )
    (repo / "tests/test_params.py").write_text(
        "import pytest\n"
        "from pkg.app import add\n"
        "\n"
        "\n"
        "@pytest.mark.parametrize('n', [0, 1])\n"
        "def test_identity(n):\n"
        "    assert add(n, 0) == n\n",
    )
    (repo / "tests/test_unittest.py").write_text(
        "import unittest\n"
        "from pkg.app import alpha\n"
        "\n"
        "\n"
        "class TestAlpha(unittest.TestCase):\n"
        "    def test_alpha_case(self):\n"
        "        self.assertEqual(alpha(), 'alpha')\n",
    )
    (repo / "tests/nested/test_nested.py").write_text(
        "from pkg.app import add\n"
        "\n"
        "\n"
        "def test_nested_add():\n"
        "    assert add(2, 2) == 4\n",
    )
    (repo / "tests/integration.rs").write_text(
        "#[test]\n"
        "fn integration_alpha() {\n"
        "    assert_eq!(kiss_timing_complex::alpha(), \"alpha\");\n"
        "}\n",
    )
    (repo / "tests/more_cases.rs").write_text(
        "mod inner {\n"
        "    #[test]\n"
        "    fn nested_add() {\n"
        "        assert_eq!(kiss_timing_complex::math::add(2, 2), 4);\n"
        "    }\n"
        "}\n",
    )
    subprocess.run(
        ["cargo", "generate-lockfile"],
        cwd=repo,
        check=True,
        capture_output=True,
        text=True,
    )
    commit_fixture_baseline(repo)


def _run_kiss_test(name: str, repo: Path, env: dict[str, str]):
    outcome = run(
        name,
        [str(KISS), "test", "."],
        repo,
        env,
        expected=0,
        timeout=50,
    )
    assert "✓ 10 passed" in outcome.stdout, (
        f"{name}: expected all allowed-style tests to run\n"
        f"stdout:\n{outcome.stdout}\nstderr:\n{outcome.stderr}"
    )
    return outcome


def timing_kiss_test() -> None:
    """Build a mixed-language tmp repo and time cold then warm `kiss test`."""
    assert KISS.is_file(), f"local binary missing: {KISS}"
    with tempfile.TemporaryDirectory(prefix="kq-test-") as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        write_complex_test_repo(repo)
        env = os.environ.copy()
        env["PYTHONPATH"] = str(repo)
        env.pop("RUSTFLAGS", None)
        cold = _run_kiss_test("kiss-test-complex-cold", repo, env)
        warm = _run_kiss_test("kiss-test-complex-warm", repo, env)
        emit_eval("kiss_test_cold_elapsed_s", "SMALLER", f"{cold.elapsed:.4f}")
        emit_eval("kiss_test_warm_elapsed_s", "SMALLER", f"{warm.elapsed:.4f}")


def eval_timing_kiss_test() -> None:
    report_eval(timing_kiss_test)
