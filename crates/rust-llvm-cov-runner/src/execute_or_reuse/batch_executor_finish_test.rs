use super::test_helpers::finish_context;
use super::{
    FreshCheckAggregateExport, build_instance_export_requests, build_instance_results,
    finish_fresh_batch_after_export, finish_fresh_check_aggregate_after_export,
};
use crate::execute_or_reuse::batch_events::{BatchCompilerArtifact, BatchTestStarted, BatchTestTerminal};
use crate::execute_or_reuse::batch_export::ExportCounters;
use crate::plan::batch_fingerprint::{batch_identity, entry_fingerprint};
use crate::plan::batch_plan::{
    CoverageOutputMode, RustCoverageBatchRequest, build_rust_coverage_batch_plan,
};
use crate::execute_or_reuse::batch_shim::BatchShimMetadata;
use crate::rust_cov_cache::load_rust_cov_cache_entry;
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, witness_batch_tools,
};
use crate::{RustCovCacheStatus, RustLineCoverage};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[test]
fn build_instance_results_and_export_requests_cover_finish_helpers() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let shim = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: "alpha".to_string(),
        full_name: "pkg::bin$alpha".to_string(),
        profile_path: std::path::PathBuf::from("/tmp/alpha.profraw"),
        cwd: repo.path().to_path_buf(),
        argv: vec!["/tmp/bin".to_string()],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
        output_frame_count: None,
    };
    let started = vec![BatchTestStarted {
        full_name: "pkg::bin$alpha".to_string(),
        test_name: "alpha".to_string(),
    }];
    let terminal = vec![BatchTestTerminal {
        full_name: "pkg::bin$alpha".to_string(),
        test_name: "alpha".to_string(),
        passed: true,
        timed_out: false,
        exec_time_secs: 0.001,
        stdout: None,
        reason: None,
    }];
    let instances = build_instance_results(
        &started,
        &[],
        &terminal,
        std::slice::from_ref(&shim),
        false,
        &req,
    )
    .expect("build_instance_results");
    assert_eq!(instances.len(), 1);
    let artifacts = vec![BatchCompilerArtifact {
        executable: Some("/tmp/bin".to_string()),
        filenames: vec!["/tmp/a.o".to_string()],
        nextest_binary_id: None,
    libtest_binary_prefix: None,
    src_path: None,
    is_test_harness: false,
    }];
    let requests =
        build_instance_export_requests(&instances, &[shim], &artifacts).expect("export requests");
    assert_eq!(requests.len(), 1);
}

#[test]
fn finish_fresh_batch_after_export_stores_completed_outcomes() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let instances = vec![crate::execute_or_reuse::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        timed_out: false,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    }];
    let exported = vec![("pkg::bin$alpha".to_string(), instances[0].coverage.clone())];
    let result = finish_fresh_batch_after_export(
        &req,
        &tools,
        &identity,
        false,
        instances,
        exported,
        ExportCounters {
            export_jobs: 1,
            max_active_exports: 1,
            max_objects_per_export: 1,
        },
        finish_context(),
    )
    .expect("finish_fresh_batch_after_export");
    assert!(result.batch_error.is_none());
    assert_eq!(result.completed.len(), 2);
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    assert!(load_rust_cov_cache_entry(&req.cache_root, &fingerprint).is_some());
}

#[test]
fn population_finish_rejects_unmatched_selectors_before_storing() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.population_publication_selectors = Some(req.logical_selectors.clone());
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let instances = vec![crate::execute_or_reuse::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        timed_out: false,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    }];
    let result = finish_fresh_batch_after_export(
        &req,
        &tools,
        &identity,
        false,
        instances,
        vec![(
            "pkg::bin$alpha".to_string(),
            RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
        )],
        ExportCounters {
            export_jobs: 1,
            max_active_exports: 1,
            max_objects_per_export: 1,
        },
        finish_context(),
    )
    .expect("finish should return a batch error result");

    assert!(result.completed.is_empty());
    assert!(matches!(
        result.batch_error,
        Some(crate::RustLlvmCovError::InvalidRequest(ref message))
            if message.contains("did not execute 1 requested Rust selector")
    ));
    assert_eq!(result.counters.unmatched_selectors, 1);
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    assert!(load_rust_cov_cache_entry(&req.cache_root, &fingerprint).is_none());
}

#[test]
fn reject_missing_terminal_events_and_instance_profile_path_are_exercised() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let started = vec![BatchTestStarted {
        full_name: "pkg::bin$alpha".to_string(),
        test_name: "alpha".to_string(),
    }];
    let err = build_instance_results(&started, &[], &[], &[], false, &req).unwrap_err();
    assert!(
        matches!(err, crate::RustLlvmCovError::InvalidRequest(message) if message.contains("missing terminal events"))
    );
    let _plan = build_rust_coverage_batch_plan(&req).unwrap();
    let _ = RustCoverageBatchRequest::witness();
}

#[test]
fn finish_fresh_check_aggregate_rejects_unmatched_selectors() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let result = finish_fresh_check_aggregate_after_export(
        &req,
        &tools,
        &identity,
        FreshCheckAggregateExport {
            exact: false,
            instances: Vec::new(),
            exported: BTreeMap::new(),
            counters: ExportCounters::default(),
        },
        finish_context(),
    )
    .expect("returns batch error result");
    assert!(result.completed.is_empty());
    assert!(matches!(
        result.batch_error,
        Some(crate::RustLlvmCovError::InvalidRequest(ref message))
            if message.contains("check aggregate batch did not execute")
    ));
}

#[test]
fn finish_fresh_check_aggregate_returns_failed_outcomes_without_publishing() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let instances = vec![crate::execute_or_reuse::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: false,
        timed_out: false,
        exit_code: Some(1),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
    }];
    let result = finish_fresh_check_aggregate_after_export(
        &req,
        &tools,
        &identity,
        FreshCheckAggregateExport {
            exact: false,
            instances,
            exported: BTreeMap::new(),
            counters: ExportCounters::default(),
        },
        finish_context(),
    )
    .expect("failed outcomes are returned");
    assert!(result.batch_error.is_none());
    assert_eq!(result.completed.len(), 1);
    assert_eq!(result.completed[0].status, TestStatus::Failed);
}

#[test]
fn finish_fresh_check_aggregate_store_failure_prevents_final_publication() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    req.cache_root = repo.path().join("cache-root-file");
    std::fs::write(&req.cache_root, "not a directory").unwrap();
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let coverage = RustLineCoverage {
        files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
    };
    let instances = vec![crate::execute_or_reuse::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        timed_out: false,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: coverage.clone(),
    }];
    let result = finish_fresh_check_aggregate_after_export(
        &req,
        &tools,
        &identity,
        FreshCheckAggregateExport {
            exact: false,
            instances,
            exported: BTreeMap::from([("/tmp/bin".to_string(), coverage)]),
            counters: ExportCounters {
                export_jobs: 1,
                max_active_exports: 1,
                max_objects_per_export: 1,
            },
        },
        finish_context(),
    )
    .expect("store failure returns batch error result");

    assert!(matches!(
        result.batch_error,
        Some(crate::RustLlvmCovError::Io(_))
    ));
    assert_eq!(
        result.completed[0].cache_status,
        RustCovCacheStatus::FreshUnstored
    );
    assert!(!req.cache_root.join("check_aggregate.json").exists());
    assert!(!req.cache_root.join("index.json").exists());
    assert!(!req.cache_root.join("population.json").exists());
}

#[test]
fn finish_fresh_check_aggregate_success_publishes_final_state_after_entries() {
    let repo = batch_executor_fixture_repo();
    std::fs::create_dir_all(repo.path().join("target")).unwrap();
    let binary_path = repo.path().join("target").join("bin-a");
    std::fs::write(&binary_path, "binary-a").unwrap();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let coverage = RustLineCoverage {
        files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
    };
    let instances = vec![crate::execute_or_reuse::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "bin-a".to_string(),
        passed: true,
        timed_out: false,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: coverage.clone(),
    }];
    let finish = super::FreshBatchFinishContext {
        export_started: std::time::Instant::now(),
        build_target_baseline_bytes: 42,
        process_residual_count: 0,
        test_binaries: vec![crate::RustTestBinaryIdentity {
            id: "bin-a".to_string(),
            executable: binary_path.to_string_lossy().to_string(),
            digest: "aaaaaaaaaaaaaaaa".to_string(),
        }],
        repair_publication: None,
    };

    let result = finish_fresh_check_aggregate_after_export(
        &req,
        &tools,
        &identity,
        FreshCheckAggregateExport {
            exact: false,
            instances,
            exported: BTreeMap::from([("bin-a".to_string(), coverage)]),
            counters: ExportCounters {
                export_jobs: 1,
                max_active_exports: 1,
                max_objects_per_export: 1,
            },
        },
        finish,
    )
    .expect("check aggregate finish should succeed");

    assert!(result.batch_error.is_none());
    assert_eq!(result.counters.aggregate_binaries, 1);
    assert_eq!(
        result.completed[0].cache_status,
        RustCovCacheStatus::MissStored
    );
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    let entry = load_rust_cov_cache_entry(&req.cache_root, &fingerprint)
        .expect("entry is readable after final publication");
    assert_eq!(entry.coverage.files["src/lib.rs"], BTreeSet::from([1]));
    assert!(req.cache_root.join("check_aggregate.json").exists());
    assert!(req.cache_root.join("index.json").exists());
    assert!(req.cache_root.join("population.json").exists());
}
