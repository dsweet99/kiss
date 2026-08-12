from pathlib import Path

from ops.qa import (
    cargo_executable_name,
    force_publication_target,
    llvm_tool_uses_single_thread,
    python_rslip_cache_root,
    sample_phase_flags,
)


def test_publication_writer_command_rust_selector_uses_dot_force_metrics() -> None:
    from ops.qa import RUST_SELECTOR_PUBLISH_ARTIFACTS, publication_writer_command

    artifact = next(iter(RUST_SELECTOR_PUBLISH_ARTIFACTS))
    cmd = publication_writer_command("rust", Path("/tmp/repo"), artifact, jobs=2)
    assert cmd[cmd.index("test") + 1] == "."
    assert "--force" in cmd
    assert "--metrics" in cmd
    assert "-j" in cmd and "2" in cmd
    assert "commit" not in cmd


def test_force_publication_target_clears_cov_records_cache(tmp_path: Path) -> None:
    """Forcing republication must invalidate the warm records cache.

    Otherwise kiss test short-circuits on cov_records_cache.json and never
    re-enters rslip/rust publication (breaking publication-crash barriers).
    """
    kiss = tmp_path / ".kiss"
    kiss.mkdir()
    records = kiss / "cov_records_cache.json"
    records.write_text("{}", encoding="utf-8")
    entries = python_rslip_cache_root(tmp_path) / "entries"
    entries.mkdir(parents=True)
    (entries / "e.json").write_text("{}", encoding="utf-8")

    force_publication_target(tmp_path, "python", "rslip_selector_entry")

    assert not records.exists(), "cov_records_cache.json must be cleared to force republication"
    assert not entries.exists()

def test_cargo_executable_name_reads_trailing_binary_name() -> None:
    assert (
        cargo_executable_name("/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest")
        == "cargo-llvm-cov"
    )


def test_llvm_tool_uses_single_thread_ignores_export_contract_package_name() -> None:
    # Regression: substring " export" matched `-p export-contract-runner`.
    command = (
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov test "
        "-p export-contract-runner -- --test-threads=1"
    )
    assert llvm_tool_uses_single_thread(command)
    build_active, test_active, export_active = sample_phase_flags([command])
    assert not export_active
    assert not build_active
    assert not test_active


def test_llvm_tool_uses_single_thread_requires_flags_on_real_export_and_merge() -> None:
    export_ok = (
        "/tool/llvm-cov export -format=text --threads=1 "
        "-instr-profile /tmp/a.profdata -object /tmp/a.o"
    )
    export_bad = (
        "/tool/llvm-cov export -format=text "
        "-instr-profile /tmp/a.profdata -object /tmp/a.o"
    )
    merge_ok = "/tool/llvm-profdata merge -sparse --num-threads=1 /tmp/a.profraw -o /tmp/a.profdata"
    merge_bad = "/tool/llvm-profdata merge -sparse /tmp/a.profraw -o /tmp/a.profdata"
    assert llvm_tool_uses_single_thread(export_ok)
    assert not llvm_tool_uses_single_thread(export_bad)
    assert llvm_tool_uses_single_thread(merge_ok)
    assert not llvm_tool_uses_single_thread(merge_bad)
    _, _, export_active = sample_phase_flags([export_ok, merge_ok])
    assert export_active


def test_observer_keeps_max_build_jobs_across_nested_invocations() -> None:
    # Nested fixture cargo-llvm-cov may report a smaller --build-jobs than kiss -j.
    observed: int | None = None
    for jobs in (32, 4, 8):
        if observed is None or jobs > observed:
            observed = jobs
    assert observed == 32


def test_cargo_build_jobs_ignores_tmp_fixture_batches_for_observer_contract() -> None:
    from ops.qa import cargo_build_jobs_from_command

    top = (
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report "
        "--build-jobs 32 --message-format libtest-json-plus "
        "--manifest-path /home/dsweet/Projects/kiss/Cargo.toml"
    )
    nested = (
        "/home/user/.cargo/bin/cargo-llvm-cov llvm-cov nextest --no-report "
        "--build-jobs 4 --message-format libtest-json-plus "
        "--manifest-path /tmp/.tmpABC/Cargo.toml"
    )
    assert cargo_build_jobs_from_command(top) == 32
    assert cargo_build_jobs_from_command(nested) == 4
    # Observer recording filter: keep top-level, drop /tmp fixtures.
    assert "libtest-json-plus" in top and "/tmp/" not in top
    assert "libtest-json-plus" in nested and "/tmp/" in nested
