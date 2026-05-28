from __future__ import annotations

from pathlib import Path

from ops.cd_python_source import infer_pytest_target


def test_infer_pytest_target_prefers_tests_dir(tmp_path: Path) -> None:
    (tmp_path / "tests").mkdir()
    (tmp_path / "ropetest").mkdir()
    assert infer_pytest_target(tmp_path) == "tests/"


def test_infer_pytest_target_falls_back_to_ropetest(tmp_path: Path) -> None:
    (tmp_path / "ropetest").mkdir()
    assert infer_pytest_target(tmp_path) == "ropetest/"


def test_infer_slipcover_source_maturin_module(tmp_path: Path) -> None:
    from ops.cd_python_source import infer_slipcover_source

    (tmp_path / "src" / "enn").mkdir(parents=True)
    (tmp_path / "src" / "enn" / "__init__.py").write_text("", encoding="utf-8")
    (tmp_path / "pyproject.toml").write_text(
        '[tool.maturin]\npython-source = "src"\nmodule-name = "enn.enn_rust"\n',
        encoding="utf-8",
    )
    assert infer_slipcover_source(tmp_path) == "src/enn"
