import python
from evals._harness import sample_phase_flags

NEXTEST_PARENT = "/home/user/.cargo/bin/cargo-nextest nextest run --workspace --profile kiss"


def test_sample_phase_flags_nextest_parent_is_not_test_execution() -> None:
    assert python.__name__ == "python"

    build_active, test_active = sample_phase_flags([NEXTEST_PARENT])
    assert not build_active
    assert not test_active


def test_sample_phase_flags_cold_compile_with_nextest_parent() -> None:
    commands = [
        NEXTEST_PARENT,
        "/home/user/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo "
        "test --no-run --message-format json-render-diagnostics",
        "/tmp/kiss-qa/target/debug/build/foo-abc/build-script-build",
    ]
    build_active, test_active = sample_phase_flags(commands)
    assert build_active
    assert not test_active


def test_sample_phase_flags_test_binary_under_nextest_is_test_execution() -> None:
    commands = [
        NEXTEST_PARENT,
        "/repo/target/debug/deps/integration-0123abcd --exact tests::alpha --nocapture",
    ]
    build_active, test_active = sample_phase_flags(commands)
    assert not build_active
    assert test_active


def test_sample_phase_flags_build_script_is_not_test_execution() -> None:
    commands = ["/repo/target/debug/build/foo-abc/build-script-build"]
    build_active, test_active = sample_phase_flags(commands)
    assert build_active
    assert not test_active


def test_sample_phase_flags_ignores_nested_tempfile_cargo_during_tests() -> None:
    commands = [
        NEXTEST_PARENT,
        "/repo/target/debug/deps/kiss-0123abcd --exact tests::alpha --nocapture",
        "/home/user/.cargo/bin/cargo test --manifest-path /tmp/.tmpAbC123/Cargo.toml "
        "--no-run --message-format json --workspace "
        "--target-dir /tmp/.tmpAbC123/target",
    ]
    build_active, test_active = sample_phase_flags(commands)
    assert not build_active
    assert test_active


def test_sample_phase_flags_still_counts_kiss_qa_fixture_compile() -> None:
    commands = [
        NEXTEST_PARENT,
        "/home/user/.cargo/bin/cargo test --manifest-path "
        "/tmp/kiss-qa-phase-abc/repo/Cargo.toml --no-run "
        "--target-dir /tmp/kiss-qa-phase-abc/repo/target",
    ]
    build_active, test_active = sample_phase_flags(commands)
    assert build_active
    assert not test_active
