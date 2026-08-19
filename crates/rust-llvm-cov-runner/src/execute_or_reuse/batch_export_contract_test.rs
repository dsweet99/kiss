
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::execute_or_reuse::batch_export_tools::resolve_export_tools_from_rustc;
use crate::execute_rust_coverage_batch;
use crate::execute_or_reuse::llvm_cov_json::parse_llvm_cov_json;

use crate::batch_export_contract_fixture::*;

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

fn run_parity_matrix_case(
    case: &ParityMatrixCase,
    tools: &crate::RustCoverageToolIdentity,
    fixture_root: &Path,
) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let helper_target = tmp.path().join(format!("helper-target-{}", case.name));
    build_helper_bin(&helper_target);
    let helper_bin = helper_bin_path(&helper_target);
    let selectors: Vec<String> = case.selectors.iter().map(|s| (*s).to_string()).collect();
    let test_args: Vec<String> = case.test_args.iter().map(|s| (*s).to_string()).collect();
    let legacy =
        collect_legacy_outcomes(&selectors, tools, &helper_bin, tmp.path(), case, &test_args);
    let batch_req =
        batch_request_with_args(tmp.path(), &selectors, &helper_bin, &test_args, case.jobs);
    let batch = execute_rust_coverage_batch(&batch_req, tools)
        .unwrap_or_else(|err| panic!("batch parity case `{}` failed: {err:?}", case.name));
    assert_parity_batch_result(case, &batch, &selectors, &legacy, &batch_req, fixture_root);
}

fn collect_legacy_outcomes(
    selectors: &[String],
    tools: &crate::RustCoverageToolIdentity,
    helper_bin: &Path,
    tmp: &Path,
    case: &ParityMatrixCase,
    test_args: &[String],
) -> BTreeMap<String, crate::RustLlvmCovOutcome> {
    selectors
        .iter()
        .map(|selector| {
            let outcome = run_legacy_selector_with_args(
                selector,
                tools,
                helper_bin,
                tmp.join(format!("legacy-cache-{}", case.name))
                    .join(selector),
                test_args,
            );
            (selector.clone(), outcome)
        })
        .collect()
}

fn assert_parity_batch_result(
    case: &ParityMatrixCase,
    batch: &crate::RustCoverageBatchResult,
    selectors: &[String],
    legacy: &BTreeMap<String, crate::RustLlvmCovOutcome>,
    batch_req: &crate::RustCoverageBatchRequest,
    fixture_root: &Path,
) {
    let debug = parity_debug(case, batch, batch_req);
    if matches!(
        case.name,
        "unmatched-selector" | "mixed-matched-unmatched" | "exact-prefix-zero-instances"
    ) {
        assert!(
            assert_parity_special_case(case, batch, selectors, legacy, batch_req, &debug),
            "unmatched-style case must be fully handled specially\n{debug}"
        );
        return;
    }
    assert!(
        batch.batch_error.is_none(),
        "batch error in case `{}`: {:?}",
        case.name,
        batch.batch_error
    );
    assert_eq!(
        batch.completed.len(),
        selectors.len(),
        "case `{}`",
        case.name
    );
    if assert_parity_special_case(case, batch, selectors, legacy, batch_req, &debug) {
        return;
    }
    assert_parity_outcomes(batch, legacy, case, fixture_root, &debug);
}

fn parity_debug(
    case: &ParityMatrixCase,
    batch: &crate::RustCoverageBatchResult,
    batch_req: &crate::RustCoverageBatchRequest,
) -> String {
    format!(
        "case={}\n{}\nbatch counters: test_instances={} unmatched_selectors={} export_jobs={} max_active_test_instances={} max_active_exports={} max_objects_per_export={}",
        case.name,
        batch_profile_debug(batch_req),
        batch.counters.test_instances,
        batch.counters.unmatched_selectors,
        batch.counters.export_jobs,
        batch.counters.max_active_test_instances,
        batch.counters.max_active_exports,
        batch.counters.max_objects_per_export
    )
}

fn assert_parity_special_case(
    case: &ParityMatrixCase,
    batch: &crate::RustCoverageBatchResult,
    selectors: &[String],
    legacy: &BTreeMap<String, crate::RustLlvmCovOutcome>,
    batch_req: &crate::RustCoverageBatchRequest,
    debug: &str,
) -> bool {
    match case.name {
        "diagnostic-exit-37" => {
            assert_diagnostic_exit_37(batch, batch_req, debug);
            true
        }
        "concurrency-bound" => {
            assert_concurrency_bound(batch, case, debug);
            false
        }
        "unmatched-selector" => {
            assert_unmatched_selector(batch, debug);
            true
        }
        "mixed-matched-unmatched" => {
            assert_mixed_matched_unmatched(batch, debug);
            true
        }
        "exact-prefix-zero-instances" => {
            assert_exact_prefix_zero_instances(batch, selectors, debug);
            true
        }
        "nocapture-live-output" => {
            assert_nocapture_live_output(batch, batch_req, debug);
            false
        }
        "suite-failure-status-agreement" => {
            assert_suite_failure_status_agreement(batch, legacy, debug);
            false
        }
        _ => false,
    }
}

fn assert_parity_outcomes(
    batch: &crate::RustCoverageBatchResult,
    legacy: &BTreeMap<String, crate::RustLlvmCovOutcome>,
    case: &ParityMatrixCase,
    fixture_root: &Path,
    debug: &str,
) {
    for batch_outcome in &batch.completed {
        let selector = &batch_outcome.selector;
        let legacy_outcome = legacy
            .get(selector)
            .unwrap_or_else(|| panic!("missing legacy outcome for {selector} in {}", case.name));
        assert_outcomes_match(selector, legacy_outcome, batch_outcome, fixture_root, debug);
    }
}

fn assert_diagnostic_exit_37(
    batch: &crate::RustCoverageBatchResult,
    batch_req: &crate::RustCoverageBatchRequest,
    debug: &str,
) {
    assert_eq!(batch.completed.len(), 1);
    let outcome = &batch.completed[0];
    assert_eq!(outcome.selector, "exits_with_diagnostic_code_37");
    assert_eq!(outcome.status, rpytest_runner::TestStatus::Failed);
    assert_eq!(outcome.exit_code, Some(1));
    assert!(outcome.coverage.files.is_empty());
    let metadata = shim_metadata_for_batch(batch_req);
    assert!(
        metadata.iter().any(|item| item.exit_code == Some(37)),
        "shim must preserve diagnostic child exit 37\n{debug}"
    );
}

fn assert_concurrency_bound(
    batch: &crate::RustCoverageBatchResult,
    case: &ParityMatrixCase,
    debug: &str,
) {
    assert!(
        batch.counters.max_active_test_instances <= case.jobs,
        "{debug}"
    );
    assert!(batch.counters.max_active_exports <= case.jobs, "{debug}");
}

fn assert_nocapture_live_output(
    _batch: &crate::RustCoverageBatchResult,
    batch_req: &crate::RustCoverageBatchRequest,
    debug: &str,
) {
    let metadata = shim_metadata_for_batch(batch_req);
    assert!(
        metadata
            .iter()
            .any(|item| item.output_frame_count.unwrap_or(0) > 0),
        "nocapture must relay live separated output frames\n{debug}"
    );
}

fn assert_suite_failure_status_agreement(
    batch: &crate::RustCoverageBatchResult,
    legacy: &BTreeMap<String, crate::RustLlvmCovOutcome>,
    debug: &str,
) {
    let failure = batch
        .completed
        .iter()
        .find(|outcome| outcome.selector == "fails_assertion_for_parity")
        .expect("failure selector outcome");
    let legacy_failure = legacy
        .get("fails_assertion_for_parity")
        .expect("legacy failure outcome");
    assert_eq!(failure.status, legacy_failure.status, "{debug}");
}

fn shim_metadata_for_batch(
    batch_req: &crate::RustCoverageBatchRequest,
) -> Vec<crate::execute_or_reuse::batch_shim::BatchShimMetadata> {
    crate::execute_or_reuse::batch_shim::load_target_runner_shim_metadata(
        &batch_req
            .generated_config
            .parent()
            .expect("run root")
            .join("instances"),
    )
    .expect("shim metadata")
}

#[test]
#[ignore = "requires cargo llvm-cov, cargo nextest, LLVM tools, and a built kiss shim"]
fn real_tool_legacy_and_batch_parity_matrix_on_fixture() {
    if !real_tool_tests_enabled() {
        return;
    }
    let _env_lock = TARGET_RUNNER_ENV_LOCK
        .lock()
        .expect("target runner env lock");
    let fixture_root = Path::new(FIXTURE_ROOT);
    let tools = real_tool_identity(fixture_root);
    let kiss_bin = build_kiss_binary();
    let _shim_guard = EnvVarGuard::set(TARGET_RUNNER_SHIM_ENV, &kiss_bin);

    for case in parity_matrix_cases() {
        run_parity_matrix_case(case, &tools, fixture_root);
    }
}

#[test]
#[ignore = "requires cargo llvm-cov, cargo nextest, LLVM tools, and a built kiss shim"]
fn real_tool_batch_leaves_repository_target_untouched() {
    if !real_tool_tests_enabled() {
        return;
    }
    let _env_lock = TARGET_RUNNER_ENV_LOCK
        .lock()
        .expect("target runner env lock");
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture_root = Path::new(FIXTURE_ROOT);
    let repo_target = fixture_root.join("target");
    fs::create_dir_all(&repo_target).expect("create repository target");
    let marker = repo_target.join("kiss-parity-target-marker");
    fs::write(&marker, b"untouched").expect("write marker");
    let before = fs::read(&marker).expect("read marker");

    let tools = real_tool_identity(fixture_root);
    let helper_target = tmp.path().join("helper-target");
    build_helper_bin(&helper_target);
    let helper_bin = helper_bin_path(&helper_target);
    let kiss_bin = build_kiss_binary();
    let _shim_guard = EnvVarGuard::set(TARGET_RUNNER_SHIM_ENV, &kiss_bin);
    let batch_req = batch_request(
        tmp.path(),
        &["invokes_helper_in_process".to_string()],
        &helper_bin,
    );
    let batch = execute_rust_coverage_batch(&batch_req, &tools).expect("execute batch coverage");
    assert!(batch.batch_error.is_none(), "{:?}", batch.batch_error);
    assert_eq!(fs::read(&marker).expect("read marker after batch"), before);
    assert!(repo_target.is_dir());
}

#[path = "batch_export_contract_unmatched_test.rs"]
mod unmatched_asserts;
use unmatched_asserts::*;
