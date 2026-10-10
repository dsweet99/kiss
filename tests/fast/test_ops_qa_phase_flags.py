import python
from evals._harness import (
    cargo_executable_name,
    sample_phase_flags,
)


def test_cargo_executable_name_reads_trailing_binary_name() -> None:
    assert python.__name__ == "python"
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
