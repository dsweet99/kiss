from ops.qa import cargo_executable_name, sample_phase_flags


def test_sample_phase_flags_detects_cargo_llvm_cov_nextest_executable_path() -> None:
    command = (
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest "
        "--no-report --build-jobs 2"
    )
    build_active, test_active, export_active = sample_phase_flags([command])
    assert not build_active
    assert test_active
    assert not export_active


def test_cargo_executable_name_reads_trailing_binary_name() -> None:
    assert (
        cargo_executable_name("/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest")
        == "cargo-llvm-cov"
    )
