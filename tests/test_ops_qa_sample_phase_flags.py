import json
import os
from pathlib import Path

import python
from ops.qa import sample_phase_flags, sample_phase_flags_with_repo


def test_sample_phase_flags_llvm_cov_nextest_parent_is_not_test_execution() -> None:
    assert python.__name__ == "python"

    command = (
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest "
        "--no-report --build-jobs 2"
    )
    build_active, test_active, export_active = sample_phase_flags([command])
    assert not build_active
    assert not test_active
    assert not export_active


def test_sample_phase_flags_cold_compile_with_nextest_parent() -> None:
    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/home/user/.cargo/bin/cargo-nextest nextest run --workspace",
        "/home/user/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo "
        "test --no-run --message-format json-render-diagnostics",
        "/tmp/kiss-qa/target/debug/build/foo-abc/build-script-build",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert build_active
    assert not test_active
    assert not export_active


def test_sample_phase_flags_warm_shim_test_with_nextest_parent() -> None:
    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/tmp/kiss/target/debug/kiss __rust-llvm-cov-target-runner /tmp/bin",
        "bash -c 'while [ ! -f /tmp/go ]; do sleep 0.01; done; "
        "KISS_RUST_LLVM_COV_DELEGATED_GO=1 exec /tmp/bin'",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert not build_active
    assert test_active
    assert not export_active


def test_sample_phase_flags_ignores_nested_tempfile_cargo_during_shim_tests() -> None:



    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/repo/target/debug/kiss __rust-llvm-cov-target-runner "
        "--output-dir /repo/.kiss/rust_llvm_cov_cache/runs/r1/instances",
        "/home/user/.cargo/bin/cargo test --manifest-path /tmp/.tmpAbC123/Cargo.toml "
        "--no-run --message-format json --workspace --jobs 32 "
        "--target-dir /tmp/.tmpAbC123/target "
        "--config /tmp/.tmpAbC123/.kiss/rust_llvm_cov_cache/runs/r/config.toml",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert not build_active
    assert test_active
    assert not export_active


def test_sample_phase_flags_ignores_kiss_export_minimal_during_shim_tests() -> None:


    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/repo/target/debug/kiss __rust-llvm-cov-target-runner "
        "--output-dir /repo/.kiss/rust_llvm_cov_cache/runs/r1/instances",
        "/home/user/.cargo/bin/cargo-llvm-cov rustc --crate-name integration "
        "--out-dir /tmp/kiss-export-minimal-3285960/llvm-cov-target/debug/deps",
        "rustc --crate-name integration "
        "--out-dir /tmp/kiss-export-minimal-3285960/llvm-cov-target/debug/deps",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert not build_active
    assert test_active
    assert not export_active


def test_sample_phase_flags_still_counts_kiss_qa_fixture_compile() -> None:
    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/home/user/.cargo/bin/cargo test --manifest-path "
        "/tmp/kiss-qa-phase-abc/repo/Cargo.toml --no-run "
        "--target-dir /tmp/kiss-qa-phase-abc/repo/target",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert build_active
    assert not test_active
    assert not export_active


def test_sample_phase_flags_export_with_nextest_parent() -> None:
    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
        "/tool/llvm-cov export -format=text --threads=1 "
        "-instr-profile /tmp/a.profdata -object /tmp/a.o",
    ]
    build_active, test_active, export_active = sample_phase_flags(commands)
    assert not build_active
    assert not test_active
    assert export_active


def test_sample_phase_flags_with_repo_arms_test_from_live_metadata(
    tmp_path: Path,
) -> None:
    cache = tmp_path / ".kiss/rust_llvm_cov_cache/runs/r1/instances"
    cache.mkdir(parents=True)
    (cache / "a.shim-start.json").write_text(
        json.dumps({"shim_identity": {"pid": os.getpid(), "pgid": os.getpgid(0)}}),
        encoding="utf-8",
    )
    commands = [
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report",
    ]
    build_active, test_active, export_active = sample_phase_flags_with_repo(
        commands, tmp_path
    )
    assert not build_active
    assert test_active
    assert not export_active
    build_active, test_active, export_active = sample_phase_flags_with_repo(
        commands, None
    )
    assert not test_active
