use super::*;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchCounters, RustLineCoverage, RustLlvmCovError,
    RustLlvmCovOutcome,
};

use crate::test_runner::last_status::prior_failures;
use crate::test_runner::rust_llvm_cov_error::format_rust_llvm_cov_error;

pub(crate) fn passed_rust_llvm_cov_outcome(selector: String) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector,
        status: rpytest_runner::TestStatus::Passed,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
        coverage: rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::new(),
        },
        test_binary_ids: vec!["test-bin".to_string()],
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

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
        "0000000000000000",
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
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::FreshUnstored,
            stdout: Some(b"fresh stdout".to_vec()),
            stderr: Some(b"fresh stderr".to_vec()),
        }],
        batch_error: Some(RustLlvmCovError::InvalidRequest(
            "late derived publication failed".to_string(),
        )),
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    let err = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap_err();

    assert!(err.contains("late derived publication failed"));
    assert_eq!(
        prior_failures(tmp.path(), kiss::Language::Rust, &identity).unwrap(),
        ["tests::failing_case"]
    );
}

#[test]
fn fresh_unstored_batch_outcome_is_counted_explicitly() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let result = RustCoverageBatchResult {
        completed: vec![RustLlvmCovOutcome {
            selector: "tests::passed_but_unstored".to_string(),
            status: rpytest_runner::TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::FreshUnstored,
            stdout: None,
            stderr: None,
        }],
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap();

    assert_eq!(summary.total, 1);
    assert_eq!(summary.cache_misses, 1);
    assert_eq!(summary.cache_unstored, 1);
}

#[test]
fn rust_selector_path_submits_one_batch_request_to_executor() {
    let tmp = tempfile::tempdir().unwrap();
    let selectors = vec!["tests::case".to_string(), "tests::other".to_string()];
    let detector_calls = Rc::new(Cell::new(0usize));
    let detector_calls_for_closure = Rc::clone(&detector_calls);
    let executor_calls = Rc::new(Cell::new(0usize));
    let executor_calls_for_closure = Rc::clone(&executor_calls);
    let expected_selectors = selectors.clone();
    let expected_repo_root = tmp.path().to_path_buf();

    let summary = run_rust_llvm_cov_selectors_with_deps(
        tmp.path(),
        &selectors,
        RustCoverageRunOptions {
            extra: &["--exact".to_string()],
            force_rerun: true,
            jobs: 7,
            population_publication_selectors: None,
            coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        },
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
                test_binaries: Vec::new(),
            })
        },
    )
    .unwrap();

    assert_eq!(detector_calls.get(), 1);
    assert_eq!(executor_calls.get(), 1);
    assert_eq!(summary.total, 2);
    assert_eq!(summary.cache_misses, 2);
}

#[test]
fn rust_selector_path_rejects_duplicate_batch_selectors_before_execution() {
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

    let err = run_rust_llvm_cov_selectors_with_deps(
        tmp.path(),
        &selectors,
        RustCoverageRunOptions {
            extra: &["--exact".to_string()],
            force_rerun: true,
            jobs: 7,
            population_publication_selectors: None,
            coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        },
        move |_repo_root| {
            detector_calls_for_closure.set(detector_calls_for_closure.get() + 1);
            Ok(RustCoverageToolVersions {
                cargo: "cargo 1.88.0".to_string(),
                llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
                rustc: "rustc 1.88.0".to_string(),
                cargo_nextest: "cargo-nextest 0.9.0".to_string(),
            })
        },
        move |_batch_req, _versions| {
            executor_calls_for_closure.set(executor_calls_for_closure.get() + 1);
            unreachable!("duplicate selectors must fail before executor starts")
        },
    )
    .unwrap_err();

    assert!(err.contains("duplicate logical selector: tests::case"));
    assert_eq!(detector_calls.get(), 0);
    assert_eq!(executor_calls.get(), 0);
}

#[test]
fn check_aggregate_population_request_carries_population_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    let selectors = vec!["tests::case".to_string()];
    let population_selectors = vec!["tests::case".to_string(), "tests::other".to_string()];
    let expected_population_selectors = population_selectors.clone();

    let summary = run_rust_llvm_cov_selectors_with_deps(
        tmp.path(),
        &selectors,
        RustCoverageRunOptions {
            extra: &[],
            force_rerun: true,
            jobs: 2,
            population_publication_selectors: Some(population_selectors),
            coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::CheckAggregate {
                publication_binary_ids: None,
                repair_publication: None,
            },
        },
        |_repo_root| {
            Ok(RustCoverageToolVersions {
                cargo: "cargo 1.88.0".to_string(),
                llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
                rustc: "rustc 1.88.0".to_string(),
                cargo_nextest: "cargo-nextest 0.9.0".to_string(),
            })
        },
        move |batch_req, _versions| {
            assert_eq!(
                batch_req.population_publication_selectors,
                Some(expected_population_selectors)
            );
            assert!(matches!(
                batch_req.coverage_output_mode,
                rust_llvm_cov_runner::CoverageOutputMode::CheckAggregate { .. }
            ));
            Ok(RustCoverageBatchResult {
                completed: batch_req
                    .logical_selectors
                    .iter()
                    .cloned()
                    .map(passed_rust_llvm_cov_outcome)
                    .collect(),
                batch_error: None,
                counters: RustCoverageBatchCounters::default(),
                test_binaries: Vec::new(),
            })
        },
    )
    .unwrap();

    assert_eq!(summary.total, selectors.len());
}

#[test]
fn check_aggregate_population_can_return_cached_summary() {
    let selectors = vec!["tests::case".to_string(), "tests::other".to_string()];
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "check-aggregate:abc".to_string(),
        selectors: selectors.clone(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };

    let summary = cached_summary_from_check_aggregate_population(&selectors, &population).unwrap();

    assert_eq!(summary.total, 2);
    assert_eq!(summary.cache_hits, 2);
    assert_eq!(summary.cache_misses, 0);

    let mut entry_backed = population;
    entry_backed.entries_fingerprint = "entry-fingerprint".to_string();
    assert!(cached_summary_from_check_aggregate_population(&selectors, &entry_backed).is_none());
}

#[test]
fn cached_check_aggregate_selectors_returns_none_without_population() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn value() {}\n").unwrap();

    let cached =
        cached_rust_check_aggregate_selectors(tmp.path(), &["tests::case".to_string()], &[])
            .unwrap();

    assert!(cached.is_none());
}

#[test]
fn rust_selector_wrappers_return_empty_summary_without_tool_detection() {
    let tmp = tempfile::tempdir().unwrap();

    let selector_summary =
        run_rust_llvm_cov_selectors(tmp.path(), &[], &[], false, 1, None).unwrap();
    let aggregate_summary =
        run_rust_llvm_cov_check_aggregate_selectors(tmp.path(), &[], &[], 1, None, None).unwrap();

    assert_eq!(selector_summary.total, 0);
    assert_eq!(aggregate_summary.total, 0);
}

#[test]
#[should_panic(expected = "jobs must be greater than zero")]
fn run_rust_llvm_cov_selectors_rejects_zero_jobs_before_spawning() {
    let tmp = tempfile::tempdir().unwrap();

    let _ = run_rust_llvm_cov_selectors(tmp.path(), &[], &[], false, 0, None);
}


#[test]
fn interrupted_error_maps_without_string_matching() {
    let err = RustLlvmCovError::Interrupted;
    assert!(matches!(err, RustLlvmCovError::Interrupted));
}
