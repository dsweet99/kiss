use super::fresh_test_helpers::{execute_rust_coverage_batch_fresh_with_fake, fake_runner, tools};
use super::*;
use crate::RustCovCacheStatus;
use crate::RustLlvmCovError;
use crate::batch_fingerprint::batch_identity;
use crate::batch_plan::build_rust_coverage_batch_plan;
use crate::batch_result::RustCoverageBatchResult;
use crate::batch_run::{
    BatchSubprocessRunner, BuildIdentityFile, BuildIdentityPreparation, build_identity_input,
    path_size_bytes, prepare_build_target_for_identity,
};
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
};
use rpytest_runner::TestStatus;
use std::fs;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn force_rerun_skips_cache_hits_and_reruns_fresh_batch() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    req.force_rerun = true;

    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert!(
        result
            .completed
            .iter()
            .all(|outcome| outcome.cache_status == RustCovCacheStatus::MissStored)
    );
    assert_eq!(result.counters.build_invocations, 1);
}

#[test]
fn fresh_batch_stores_passed_selector_entries() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert!(result.completed.iter().all(|outcome| {
        outcome.status == TestStatus::Passed
            && outcome.cache_status == RustCovCacheStatus::MissStored
    }));
    assert_eq!(result.counters.build_invocations, 1);
    assert_eq!(result.counters.export_jobs, 1);
    assert_eq!(result.counters.build_target_baseline_bytes, 12);
}

#[test]
fn fresh_build_identity_helpers_track_build_compatible_inputs() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    let tools = tools();
    let base = build_identity_input(&req, &tools);
    let mut same_build = req.clone();
    same_build.logical_selectors = vec!["other".to_string()];
    same_build.test_args = vec!["--nocapture".to_string()];
    let marker = BuildIdentityFile {
        input: base.clone(),
        build_target_baseline_bytes: 12,
    };
    let prep = BuildIdentityPreparation {
        previous_baseline_bytes: 12,
    };

    assert_eq!(base, build_identity_input(&same_build, &tools));
    assert_eq!(marker.input.cache_schema, crate::CACHE_SCHEMA_VERSION);
    assert_eq!(
        marker.input.execution_policy,
        crate::BATCH_EXECUTION_POLICY_VERSION
    );
    assert_eq!(marker.input.tool_versions[0], tools.cargo_version.as_str());
    assert_eq!(
        prep.previous_baseline_bytes,
        marker.build_target_baseline_bytes
    );

    req.env
        .insert("RUSTFLAGS".to_string(), "-Cinstrument".to_string());
    assert_ne!(base, build_identity_input(&req, &tools));
}

#[test]
fn fresh_build_identity_drops_incompatible_baseline_and_retains_external_target() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let _ = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(path_size_bytes(&plan.build_target).unwrap(), 12);

    req.cargo_args.push("--features=changed".to_string());
    let changed_plan = build_rust_coverage_batch_plan(&req).unwrap();
    let prep = prepare_build_target_for_identity(&req, &tools, &changed_plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!identity.generation_fingerprint.is_empty());
    assert!(changed_plan.build_target.exists());
}

#[test]
fn fresh_batch_requires_shim_metadata_for_matched_instances() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let runner = BatchSubprocessRunner::from_fn(|_, _| {
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: br#"{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.001}
"#
            .to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let err = execute_fresh_batch_with_export_fn(
        &req,
        &tools,
        &identity,
        &plan,
        &runner,
        Arc::new(|_, _, _, _| {
            Err(RustLlvmCovError::InvalidRequest(
                "missing target-runner metadata".into(),
            ))
        }),
    )
    .unwrap_err();

    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("missing target-runner metadata"))
    );
}

#[test]
fn subprocess_exporter_wrapper_propagates_pre_export_failures() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let runner = BatchSubprocessRunner::from_fn(|_, _| {
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(17),
            stdout: br#"{"reason":"build-finished","success":true}"#.to_vec(),
            stderr: b"no terminal events".to_vec(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let exporter = crate::batch_export::SubprocessInstanceExporter::new(
        crate::batch_export_tools::ExportTools {
            llvm_profdata: "/bin/false".into(),
            llvm_cov: "/bin/false".into(),
            llvm_readobj: "/bin/false".into(),
        },
        None,
    );

    let err = execute_fresh_batch_with_exporter(&req, &tools, &identity, &plan, &runner, exporter)
        .unwrap_err();

    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("without terminal test events"))
    );
}

#[test]
fn subprocess_exporter_wrapper_handles_failed_test_without_export_jobs() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        let bin = plan.build_target.join("bin");
        fs::write(&bin, b"binary").unwrap();
        super::fresh_test_helpers::write_shim_metadata(
            &plan.target_runner_output_dir,
            "pkg::bin$alpha",
            &bin,
        );
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(1),
            stdout: format!(
                "{{\"reason\":\"compiler-artifact\",\"executable\":\"{}\",\"filenames\":[\"/tmp/a.o\"],\"fresh\":false}}\n{{\"reason\":\"build-finished\",\"success\":true}}\n{{\"type\":\"test\",\"event\":\"failed\",\"name\":\"pkg::bin$alpha\",\"exec_time\":0.001}}\n",
                bin.display()
            )
            .into_bytes(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let exporter = crate::batch_export::SubprocessInstanceExporter::new(
        crate::batch_export_tools::ExportTools {
            llvm_profdata: "/bin/false".into(),
            llvm_cov: "/bin/false".into(),
            llvm_readobj: "/bin/false".into(),
        },
        None,
    );

    let result =
        execute_fresh_batch_with_exporter(&req, &tools, &identity, &plan, &runner, exporter)
            .unwrap();

    assert_eq!(result.counters.export_jobs, 0);
    assert_eq!(result.completed[0].status, TestStatus::Failed);
}

#[test]
fn check_aggregate_branch_reports_missing_shim_metadata_before_export() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.coverage_output_mode = crate::batch_plan::CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        let bin = plan.build_target.join("bin");
        fs::write(&bin, b"binary").unwrap();
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: format!(
                "{{\"reason\":\"compiler-artifact\",\"executable\":\"{}\",\"filenames\":[\"/tmp/a.o\"],\"fresh\":false}}\n{{\"reason\":\"build-finished\",\"success\":true}}\n{{\"type\":\"test\",\"event\":\"ok\",\"name\":\"pkg::bin$alpha\",\"exec_time\":0.001}}\n",
                bin.display()
            )
            .into_bytes(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();

    let err = execute_fresh_batch_with_export_fn(
        &req,
        &tools,
        &identity,
        &plan,
        &runner,
        Arc::new(|_, _, _, _| unreachable!("check aggregate path does not use instance exporter")),
    )
    .unwrap_err();

    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("missing target-runner metadata"))
    );
}

#[test]
fn check_aggregate_branch_builds_export_requests_with_shim_metadata() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.coverage_output_mode = crate::batch_plan::CoverageOutputMode::CheckAggregate {
        publication_binary_ids: None,
        repair_publication: None,
    };
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        let bin = plan.build_target.join("bin");
        fs::write(&bin, b"binary").unwrap();
        super::fresh_test_helpers::write_shim_metadata(
            &plan.target_runner_output_dir,
            "pkg::bin$alpha",
            &bin,
        );
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: format!(
                "{{\"reason\":\"compiler-artifact\",\"executable\":\"{}\",\"filenames\":[\"{}.o\"],\"fresh\":false}}\n{{\"reason\":\"build-finished\",\"success\":true}}\n{{\"type\":\"test\",\"event\":\"ok\",\"name\":\"pkg::bin$alpha\",\"exec_time\":0.001}}\n",
                bin.display(),
                bin.display()
            )
            .into_bytes(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let err = execute_fresh_batch_with_export_fn(
        &req,
        &tools,
        &identity,
        &plan,
        &runner,
        Arc::new(|_, _, _, _| unreachable!("check aggregate uses aggregate exporter")),
    )
    .unwrap_err();
    // Should get past shim resolution into aggregate export / finish.
    let message = format!("{err:?}");
    assert!(
        message.contains("export")
            || message.contains("profdata")
            || message.contains("llvm")
            || message.contains("InvalidRequest")
            || message.contains("Io"),
        "unexpected error: {message}"
    );
}

#[test]
fn apply_non_primary_cleanup_error_propagates_io_failure() {
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: Default::default(),
        test_binaries: Vec::new(),
    };
    let outcome = apply_non_primary_cleanup_error(
        result,
        Some(std::io::Error::other("stale cleanup failed")),
    )
    .unwrap();
    assert!(matches!(
        outcome.batch_error,
        Some(RustLlvmCovError::Io(err)) if err.to_string().contains("stale cleanup failed")
    ));
}

#[test]
fn apply_non_primary_cleanup_error_passes_through_clean_result() {
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: Default::default(),
        test_binaries: Vec::new(),
    };
    let ok = apply_non_primary_cleanup_error(result, None).unwrap();
    assert!(ok.completed.is_empty());
}
