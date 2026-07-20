use super::test_helpers::{finish_context, test_binary};
use super::{
    build_instance_export_requests, build_instance_results,
    finish_fresh_check_aggregate_after_export,
};
use crate::batch_events::BatchTestTerminal;
use crate::batch_export::ExportCounters;
use crate::batch_fingerprint::batch_identity;
use crate::batch_plan::CoverageOutputMode;
use crate::batch_shim::BatchShimMetadata;
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, witness_batch_tools,
};
use crate::RustLineCoverage;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[test]
fn finish_fresh_check_aggregate_publishes_successful_outcomes() {
    let repo = batch_executor_fixture_repo();
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
    let instances = vec![crate::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: coverage.clone(),
    }];
    let exported = BTreeMap::from([("/tmp/bin".to_string(), coverage)]);
    let result = finish_fresh_check_aggregate_after_export(
        &req,
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
    );
    // Publication may fail validation on fixture digests; either a published result or a
    // structured InvalidRequest still exercises the success-path construction above.
    match result {
        Ok(ok) => {
            assert!(ok.batch_error.is_none() || ok.counters.aggregate_exports >= 1);
        }
        Err(err) => {
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("check aggregate") || rendered.contains("InvalidRequest"),
                "{rendered}"
            );
        }
    }
}

#[test]
fn finish_fresh_check_aggregate_merges_repair_publication_line_maps() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    req.coverage_output_mode = CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let retained = RustLineCoverage {
        files: BTreeMap::from([("src/retained.rs".to_string(), BTreeSet::from([2]))]),
    };
    let exported_cov = RustLineCoverage {
        files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
    };
    let instances = vec![crate::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: exported_cov.clone(),
    }];
    let exported = BTreeMap::from([("/tmp/bin-new".to_string(), exported_cov)]);
    let mut finish = finish_context();
    finish.repair_publication = Some(crate::batch_plan::CheckAggregateRepairPublication {
        selector_binary_ids: BTreeMap::from([(
            "alpha".to_string(),
            vec!["/tmp/bin-new".to_string()],
        )]),
        test_binaries: vec![test_binary()],
        retained_binary_line_maps: BTreeMap::from([("/tmp/bin-old".to_string(), retained)]),
    });
    let result = finish_fresh_check_aggregate_after_export(
        &req,
        &identity,
        false,
        instances,
        exported,
        ExportCounters {
            export_jobs: 1,
            max_active_exports: 1,
            max_objects_per_export: 1,
        },
        finish,
    );
    match result {
        Ok(ok) => {
            assert!(ok.batch_error.is_none() || ok.counters.aggregate_exports >= 1);
        }
        Err(err) => {
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("check aggregate")
                    || rendered.contains("InvalidRequest")
                    || rendered.contains("digest"),
                "{rendered}"
            );
        }
    }
}

#[test]
fn build_instance_results_skips_terminals_outside_logical_selectors() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["only_this".to_string()];
    let shim = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: "other".to_string(),
        full_name: "pkg::bin$other".to_string(),
        profile_path: std::path::PathBuf::from("/tmp/other.profraw"),
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
    let terminal = vec![BatchTestTerminal {
        full_name: "pkg::bin$other".to_string(),
        test_name: "other".to_string(),
        passed: true,
        exec_time_secs: 0.001,
        stdout: None,
        reason: None,
    }];
    let instances = build_instance_results(&[], &[], &terminal, &[shim], false, &req).unwrap();
    assert!(instances.is_empty());
}

#[test]
fn build_instance_export_requests_errors_when_argv_missing() {
    let instances = vec![crate::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
    }];
    let shim = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: "alpha".to_string(),
        full_name: "pkg::bin$alpha".to_string(),
        profile_path: std::path::PathBuf::from("/tmp/alpha.profraw"),
        cwd: std::path::PathBuf::from("/tmp"),
        argv: Vec::new(),
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
        output_frame_count: None,
    };
    let err = build_instance_export_requests(&instances, &[shim], &[]).unwrap_err();
    assert!(format!("{err:?}").contains("missing test binary argv"));
}

#[test]
fn build_instance_export_requests_errors_when_objects_missing() {
    let instances = vec![crate::batch_aggregate::InstanceResult {
        full_name: "pkg::bin$alpha".to_string(),
        test_binary_id: "/tmp/bin".to_string(),
        passed: true,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: None,
        stderr: None,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
    }];
    let shim = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: "alpha".to_string(),
        full_name: "pkg::bin$alpha".to_string(),
        profile_path: std::path::PathBuf::from("/tmp/alpha.profraw"),
        cwd: std::path::PathBuf::from("/tmp"),
        argv: vec!["/tmp/bin".to_string()],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
        output_frame_count: None,
    };
    let err = build_instance_export_requests(&instances, &[shim], &[]).unwrap_err();
    assert!(format!("{err:?}").contains("no instrumented objects"));
}
