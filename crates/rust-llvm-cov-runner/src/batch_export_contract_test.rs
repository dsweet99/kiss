//! Real-tool contract: direct per-instance export must match `cargo llvm-cov --json`.
//!
//! Run manually: `KISS_REAL_TOOL_TESTS=1 cargo nextest run -p rust-llvm-cov-runner real_tool_direct_export_matches_cargo_llvm_cov_json`

use std::collections::BTreeMap;
use std::path::Path;

use crate::batch_export_tools::resolve_export_tools_from_rustc;
use crate::execute_rust_coverage_batch;
use crate::llvm_cov_json::parse_llvm_cov_json;

use crate::batch_export_contract_fixture::{
    EnvVarGuard, FIXTURE_ROOT, TARGET_RUNNER_ENV_LOCK, TARGET_RUNNER_SHIM_ENV,
    assert_direct_export_matches_oracle, assert_export_uses_seed_objects_only,
    assert_outcomes_match, batch_profile_debug, batch_request, build_helper_bin, build_kiss_binary,
    collect_object_files, discover_compiler_artifacts, discover_integration_test_executable,
    discover_profraw_files, discover_seed_objects, export_merged_profile,
    fixture_relative_coverage, helper_bin_path, merge_profraws_for_test, real_tool_identity,
    real_tool_tests_enabled, run_cargo_llvm_cov_json, run_legacy_selector,
};

#[test]
#[ignore = "requires cargo llvm-cov and LLVM tools; set KISS_REAL_TOOL_TESTS=1"]
fn real_tool_direct_export_matches_cargo_llvm_cov_json() {
    if !real_tool_tests_enabled() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let target_dir = tmp.path().join("target");
    let fixture_root = Path::new(FIXTURE_ROOT);
    let source_root = fixture_root.join("runner");

    let oracle_bytes = run_cargo_llvm_cov_json(&target_dir);
    let oracle = parse_llvm_cov_json(&oracle_bytes, &source_root).expect("parse oracle json");
    let oracle_lines = fixture_relative_coverage(&oracle, fixture_root);

    let profraws = discover_profraw_files(&target_dir);
    assert!(!profraws.is_empty(), "expected profraw profiles");
    let executable = discover_integration_test_executable(&target_dir);
    let seeds = discover_seed_objects(&target_dir, &executable);
    let artifacts = discover_compiler_artifacts(&executable, &seeds);
    let mut catalog = Vec::new();
    collect_object_files(&target_dir, &mut catalog);
    catalog.sort();
    catalog.dedup();
    assert_export_uses_seed_objects_only(&artifacts, &executable, catalog.len());

    let tools = resolve_export_tools_from_rustc(std::ffi::OsStr::new("rustc")).expect("tools");
    let profdata = tmp.path().join("merged.profdata");
    merge_profraws_for_test(&tools, &profraws, &profdata).expect("merge profraws");
    let direct = export_merged_profile(&tools, &profdata, &source_root, &seeds).expect("export");
    let direct_lines = fixture_relative_coverage(&direct, fixture_root);
    assert_direct_export_matches_oracle(&oracle_lines, &direct_lines, &seeds, catalog.len());
}

#[test]
#[ignore = "requires cargo llvm-cov, cargo nextest, LLVM tools, and a built kiss shim"]
fn real_tool_legacy_and_batch_outputs_match_on_fixture() {
    if !real_tool_tests_enabled() {
        return;
    }
    let _env_lock = TARGET_RUNNER_ENV_LOCK
        .lock()
        .expect("target runner env lock");
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture_root = Path::new(FIXTURE_ROOT);
    let tools = real_tool_identity(fixture_root);
    let helper_target = tmp.path().join("helper-target");
    build_helper_bin(&helper_target);
    let helper_bin = helper_bin_path(&helper_target);
    assert!(
        helper_bin.is_file(),
        "helper bin missing: {}",
        helper_bin.display()
    );

    let selectors = vec![
        "invokes_helper_in_process".to_string(),
        "spawns_instrumented_helper_binary".to_string(),
    ];
    let legacy: BTreeMap<_, _> = selectors
        .iter()
        .map(|selector| {
            let outcome = run_legacy_selector(
                selector,
                &tools,
                &helper_bin,
                tmp.path().join("legacy-cache").join(selector),
            );
            (selector.clone(), outcome)
        })
        .collect();

    let kiss_bin = build_kiss_binary();
    let _shim_guard = EnvVarGuard::set(TARGET_RUNNER_SHIM_ENV, &kiss_bin);
    let batch_req = batch_request(tmp.path(), &selectors, &helper_bin);
    let batch = execute_rust_coverage_batch(&batch_req, &tools).expect("execute batch coverage");
    assert!(
        batch.batch_error.is_none(),
        "batch error: {:?}",
        batch.batch_error
    );
    assert_eq!(batch.completed.len(), selectors.len());
    let debug = format!(
        "{}\nbatch counters: test_instances={} unmatched_selectors={} export_jobs={} max_objects_per_export={}",
        batch_profile_debug(&batch_req),
        batch.counters.test_instances,
        batch.counters.unmatched_selectors,
        batch.counters.export_jobs,
        batch.counters.max_objects_per_export
    );

    for batch_outcome in &batch.completed {
        let selector = &batch_outcome.selector;
        let legacy_outcome = legacy
            .get(selector)
            .unwrap_or_else(|| panic!("missing legacy outcome for {selector}"));
        assert_outcomes_match(
            selector,
            legacy_outcome,
            batch_outcome,
            fixture_root,
            &debug,
        );
    }
}
