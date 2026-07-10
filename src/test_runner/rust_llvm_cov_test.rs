use super::*;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use rust_llvm_cov_runner::{RustCoverageBatchCounters, RustLineCoverage};

use crate::test_runner::last_status::prior_failures;

#[test]
fn format_rust_llvm_cov_error_preserves_context_and_message() {
    let msg =
        format_rust_llvm_cov_error(RustLlvmCovError::InvalidRequest("bad selector".to_string()));

    assert!(msg.contains("rust llvm-cov failed"));
    assert!(msg.contains("bad selector"));
}

#[test]
fn batch_result_records_completed_outcomes_before_returning_late_error() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
    );
    let result = RustCoverageBatchResult {
        completed: vec![RustLlvmCovOutcome {
            selector: "tests::failing_case".to_string(),
            status: rpytest_runner::TestStatus::Failed,
            exit_code: Some(1),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: RustCovCacheStatus::FreshUnstored,
            stdout: Some(b"fresh stdout".to_vec()),
            stderr: Some(b"fresh stderr".to_vec()),
        }],
        batch_error: Some(RustLlvmCovError::InvalidRequest(
            "late derived publication failed".to_string(),
        )),
        counters: RustCoverageBatchCounters::default(),
    };

    let err = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap_err();

    assert!(err.contains("late derived publication failed"));
    assert_eq!(
        prior_failures(tmp.path(), kiss::Language::Rust, &identity).unwrap(),
        ["tests::failing_case"]
    );
}

#[test]
fn rust_selector_path_submits_one_batch_request_to_executor() {
    let tmp = tempfile::tempdir().unwrap();
    let selectors = vec![
        "tests::case".to_string(),
        "tests::case".to_string(),
        "tests::other".to_string(),
    ];
    let detector_calls = Rc::new(Cell::new(0usize));
    let detector_calls_for_closure = Rc::clone(&detector_calls);
    let executor_calls = Rc::new(Cell::new(0usize));
    let executor_calls_for_closure = Rc::clone(&executor_calls);
    let expected_selectors = selectors.clone();
    let expected_repo_root = tmp.path().to_path_buf();

    let summary = run_rust_llvm_cov_selectors_with_deps(
        tmp.path(),
        &selectors,
        &["--exact".to_string()],
        true,
        7,
        move |repo_root| {
            detector_calls_for_closure.set(detector_calls_for_closure.get() + 1);
            assert_eq!(repo_root, expected_repo_root);
            Ok(RustCoverageToolVersions {
                cargo: "cargo 1.88.0".to_string(),
                llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
                rustc: "rustc 1.88.0".to_string(),
                cargo_nextest: "cargo-nextest 0.9.0".to_string(),
            })
        },
        move |batch_req, versions| {
            executor_calls_for_closure.set(executor_calls_for_closure.get() + 1);
            assert_eq!(batch_req.logical_selectors, expected_selectors);
            assert_eq!(batch_req.test_args, ["--exact"]);
            assert_eq!(batch_req.jobs, 7);
            assert!(batch_req.force_rerun);
            assert_eq!(versions.llvm_cov, "cargo-llvm-cov 0.6.0");
            Ok(RustCoverageBatchResult {
                completed: batch_req
                    .logical_selectors
                    .iter()
                    .cloned()
                    .map(passed_rust_llvm_cov_outcome)
                    .collect(),
                batch_error: None,
                counters: RustCoverageBatchCounters::default(),
            })
        },
    )
    .unwrap();

    assert_eq!(detector_calls.get(), 1);
    assert_eq!(executor_calls.get(), 1);
    assert_eq!(summary.total, 3);
    assert_eq!(summary.cache_misses, 3);
}

#[test]
#[should_panic(expected = "jobs must be greater than zero")]
fn run_rust_llvm_cov_selectors_rejects_zero_jobs_before_spawning() {
    let tmp = tempfile::tempdir().unwrap();

    let _ = run_rust_llvm_cov_selectors(tmp.path(), &[], &[], false, 0);
}

#[test]
fn run_rust_llvm_cov_selectors_rejects_unsupported_test_args_before_tool_detection() {
    let tmp = tempfile::tempdir().unwrap();

    let err = run_rust_llvm_cov_selectors(
        tmp.path(),
        &["tests::case".to_string()],
        &["--format".to_string(), "json".to_string()],
        false,
        1,
    )
    .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--format"));
}

#[test]
fn rust_llvm_cov_request_contract_preserves_selector_and_cache_root() {
    let tmp = tempfile::tempdir().unwrap();
    let extra = vec!["--exact".to_string()];
    let req = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "tests::case",
        &extra,
        "llvm-cov 0.6.0",
        "rustc 1.88.0",
        true,
    )
    .unwrap();

    assert_eq!(req.selector, "tests::case");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.test_args, extra);
    assert!(req.force_rerun);
    assert!(req.cache_root.ends_with("rust_llvm_cov_cache"));
}

#[test]
fn rust_llvm_cov_request_from_batch_parts_preserves_batch_execution_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let batch_req = rust_coverage_batch_request_from_parts(
        tmp.path(),
        &["tests::case".to_string()],
        &["--exact".to_string()],
        true,
        3,
    )
    .unwrap();

    let req = rust_llvm_cov_request_from_batch_parts(
        &batch_req,
        "tests::case",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
    )
    .unwrap();

    assert_eq!(req.selector, "tests::case");
    assert_eq!(req.cwd, batch_req.cwd);
    assert_eq!(req.source_root, batch_req.source_root);
    assert_eq!(req.cargo, batch_req.cargo);
    assert_eq!(req.test_args, batch_req.test_args);
    assert_eq!(req.cache_root, batch_req.cache_root);
    assert!(req.force_rerun);
    assert_eq!(req.worker_slot, 0);
}

#[test]
fn rust_coverage_batch_request_from_parts_preserves_selector_occurrences() {
    let tmp = tempfile::tempdir().unwrap();
    let selectors = vec![
        "tests::case".to_string(),
        "tests::case".to_string(),
        "tests::other".to_string(),
    ];
    let extra = vec!["--exact".to_string()];

    let req =
        rust_coverage_batch_request_from_parts(tmp.path(), &selectors, &extra, true, 3).unwrap();

    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.source_root, tmp.path());
    assert_eq!(
        req.cache_root,
        tmp.path().join(".kiss").join("rust_llvm_cov_cache")
    );
    assert_eq!(req.logical_selectors, selectors);
    assert_eq!(req.test_args, extra);
    assert!(req.force_rerun);
    assert_eq!(req.jobs, 3);
    assert_eq!(
        req.generated_config,
        tmp.path()
            .join(".kiss")
            .join("rust_llvm_cov_cache")
            .join("runs")
            .join("nextest.toml")
    );
}

#[test]
fn compatibility_batch_executor_returns_batch_error_without_completed_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut batch_req = rust_coverage_batch_request_from_parts(
        tmp.path(),
        &["tests::case".to_string()],
        &[],
        true,
        1,
    )
    .unwrap();
    batch_req.cargo = "/definitely/not/cargo".into();
    let versions = RustCoverageToolVersions {
        cargo: "cargo 1.88.0".to_string(),
        llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
        rustc: "rustc 1.88.0".to_string(),
        cargo_nextest: "cargo-nextest 0.9.0".to_string(),
    };

    let result = execute_rust_coverage_batch_compat(&batch_req, &versions).unwrap();

    assert!(result.completed.is_empty());
    assert!(matches!(
        result.batch_error,
        Some(RustLlvmCovError::Runner(_))
    ));
}

#[test]
fn detect_rust_coverage_tool_versions_reports_installed_tools() {
    let versions = detect_rust_coverage_tool_versions(Path::new(env!("CARGO_MANIFEST_DIR")))
        .expect("Rust coverage tools are installed for the quality gates");

    assert!(versions.cargo.contains("cargo"));
    assert!(versions.llvm_cov.contains("cargo-llvm-cov"));
    assert!(versions.rustc.contains("rustc"));
    assert!(versions.cargo_nextest.contains("cargo-nextest"));
}

#[test]
fn rust_llvm_cov_request_rejects_unsupported_test_args() {
    let tmp = tempfile::tempdir().unwrap();
    let err = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "tests::case",
        &["--test-threads".to_string(), "8".to_string()],
        "llvm-cov 0.6.0",
        "rustc 1.88.0",
        false,
    )
    .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--test-threads"));
}

#[test]
fn print_rust_llvm_cov_outcome_accepts_all_status_cache_shapes() {
    for (status, cache_status) in [
        (rpytest_runner::TestStatus::Passed, RustCovCacheStatus::Hit),
        (
            rpytest_runner::TestStatus::Passed,
            RustCovCacheStatus::MissStored,
        ),
        (
            rpytest_runner::TestStatus::Passed,
            RustCovCacheStatus::FreshUnstored,
        ),
        (rpytest_runner::TestStatus::Failed, RustCovCacheStatus::Hit),
        (
            rpytest_runner::TestStatus::Failed,
            RustCovCacheStatus::MissStored,
        ),
        (
            rpytest_runner::TestStatus::Failed,
            RustCovCacheStatus::FreshUnstored,
        ),
    ] {
        print_rust_llvm_cov_outcome(&RustLlvmCovOutcome {
            selector: "tests::case".to_string(),
            status,
            exit_code: Some(i32::from(status == rpytest_runner::TestStatus::Failed)),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status,
            stdout: None,
            stderr: Some(Vec::new()),
        });
    }
}
