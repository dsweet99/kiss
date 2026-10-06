from pathlib import Path

import python
from evals._harness import (
    cargo_executable_name,
    force_publication_target,
    publication_writer_command,
    sample_phase_flags,
)


def test_publication_writer_command_python_forces_whole_repo() -> None:
    assert python.__name__ == "python"

    cmd = publication_writer_command("python", Path("/tmp/repo"), "test_record", jobs=2)
    test_idx = cmd.index("test")
    assert cmd[test_idx + 1] == "."
    assert cmd[-2:] == ["-j", "2"]
    assert "--metrics" not in cmd


def test_force_publication_target_clears_cov_records_cache(tmp_path: Path) -> None:
    kiss = tmp_path / ".kiss" / "test"
    kiss.mkdir(parents=True)
    records = kiss / "cov_records_cache.json"
    records.write_text("{}", encoding="utf-8")
    entries = tmp_path / ".kiss" / "test" / "records" / "python"
    entries.mkdir(parents=True)
    (entries / "e.json").write_text("{}", encoding="utf-8")

    force_publication_target(tmp_path, "python", "test_record")

    assert not records.exists(), "cov_records_cache.json must be cleared to force republication"
    assert not entries.exists()


def test_cargo_executable_name_reads_trailing_binary_name() -> None:
    assert (
        cargo_executable_name("/home/user/.cargo/bin/cargo-nextest nextest run")
        == "cargo-nextest"
    )


def test_cargo_test_package_named_like_a_tool_is_not_a_phase() -> None:
    command = (
        "/home/user/.cargo/bin/cargo-nextest nextest run "
        "-p export-contract-runner -- --test-threads=1"
    )
    build_active, test_active = sample_phase_flags([command])
    assert not build_active
    assert not test_active
