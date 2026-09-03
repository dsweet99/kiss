use super::fresh_test_helpers::{
    execute_rust_coverage_batch_fresh_with_fake, fake_runner, run_root_for, tools,
    write_shim_metadata,
};
use super::*;
use crate::rust_llvm_cov_runner::RustLineCoverage;
use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_fresh::{
    execute_fresh_batch_with_export_fn, execute_fresh_batch_with_export_fn_and_cleanup,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::FakeInstanceExporter;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::{
    BatchSubprocessRunner, CurrentRunCleanup, prepare_batch_run_layout,
};
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity;
use crate::rust_llvm_cov_runner::plan::batch_plan::build_rust_coverage_batch_plan;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn seed_durable_cache_artifacts(cache_root: &Path) {
    fs::create_dir_all(cache_root.join("entries")).unwrap();
    fs::write(cache_root.join("index.json"), b"[]").unwrap();
    fs::write(cache_root.join("population.json"), b"{}").unwrap();
    fs::create_dir_all(cache_root.join("locks")).unwrap();
    fs::write(cache_root.join("locks").join("batch.lock"), b"").unwrap();
}

fn assert_durable_cache_artifacts_intact(cache_root: &Path) {
    assert!(cache_root.join("index.json").is_file());
    assert!(cache_root.join("population.json").is_file());
    assert!(cache_root.join("locks").join("batch.lock").is_file());
    assert!(cache_root.join("entries").is_dir());
}

#[test]
fn successful_fresh_execution_removes_current_run_directory() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let cache_root = req.cache_root.clone();
    seed_durable_cache_artifacts(&cache_root);
    let run_root = run_root_for(&req);
    let build_target = cache_root.join("build").join("target");

    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert!(result.batch_error.is_none());
    assert!(!run_root.exists());
    assert!(build_target.is_dir());
    assert_durable_cache_artifacts_intact(&cache_root);
}

#[test]
fn reject_nonzero_without_terminal_events_direct_witness() {
    use crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchEventStream;
    use crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::BatchSubprocessRunOutcome;

    let ok_zero = reject_nonzero_without_terminal_events(
        &BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        },
        &BatchEventStream::default(),
    );
    assert!(ok_zero.is_ok());

    let ok_terminal = reject_nonzero_without_terminal_events(
        &BatchSubprocessRunOutcome {
            exit_code: Some(9),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        },
        &BatchEventStream {
            terminal_tests: vec![
                crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestTerminal {
                    full_name: "pkg::bin$alpha".to_string(),
                    test_name: "alpha".to_string(),
                    passed: true,
                    timed_out: false,
                    exec_time_secs: 0.001,
                    stdout: None,
                    reason: None,
                },
            ],
            ..Default::default()
        },
    );
    assert!(ok_terminal.is_ok());

    let err = reject_nonzero_without_terminal_events(
        &BatchSubprocessRunOutcome {
            exit_code: Some(9),
            stdout: Vec::new(),
            stderr: b"missing".to_vec(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        },
        &BatchEventStream::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("without terminal test events"))
    );
}

#[test]
fn fresh_batch_rejects_nonzero_exit_without_terminal_events() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        Ok(
            crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::BatchSubprocessRunOutcome {
                exit_code: Some(17),
                stdout: br#"{"reason":"build-finished","success":true}"#.to_vec(),
                stderr: b"no terminal events".to_vec(),
                duration: Duration::from_millis(1),
                process_residual_count: 0,
            },
        )
    });
    let err = execute_rust_coverage_batch_fresh_with_fake(&req, runner).unwrap_err();
    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("without terminal test events"))
    );
    assert!(!run_root.exists());
}

#[test]
fn fresh_batch_allows_nonzero_exit_when_terminal_events_exist() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.logical_selectors = vec!["alpha".to_string()];
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        let bin = plan.build_target.join("bin");
        fs::write(&bin, b"binary").unwrap();
        write_shim_metadata(&plan.target_runner_output_dir, "pkg::bin$alpha", &bin);
        Ok(crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::BatchSubprocessRunOutcome {
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
    let result = execute_rust_coverage_batch_fresh_with_fake(&req, runner).unwrap();
    assert!(
        result
            .completed
            .iter()
            .any(|outcome| outcome.selector == "alpha")
    );
}

#[test]
fn build_failure_attempts_current_run_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        Ok(
            crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::BatchSubprocessRunOutcome {
                exit_code: Some(0),
                stdout: br#"{"reason":"build-finished","success":false}"#.to_vec(),
                stderr: b"build failed".to_vec(),
                duration: Duration::from_millis(1),
                process_residual_count: 0,
            },
        )
    });
    let err = execute_rust_coverage_batch_fresh_with_fake(&req, runner).unwrap_err();
    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("build failed"))
    );
    assert!(!run_root.exists());
}

#[test]
fn export_failure_attempts_current_run_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let err = execute_fresh_batch_with_export_fn(
        &req,
        &tools,
        &identity,
        &plan,
        &fake_runner(),
        Arc::new(|_, _, _, _| Err(RustLlvmCovError::InvalidRequest("export failed".into()))),
    )
    .unwrap_err();
    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("export failed"))
    );
    assert!(!run_root.exists());
}

#[test]
fn store_failure_attempts_current_run_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    seed_durable_cache_artifacts(&req.cache_root);
    fs::create_dir_all(req.cache_root.join("entries")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            req.cache_root.join("entries"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
    }
    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert!(result.batch_error.is_some());
    assert!(!run_root.exists());
    assert_durable_cache_artifacts_intact(&req.cache_root);
}

#[test]
fn interrupted_batch_attempts_current_run_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    let runner = BatchSubprocessRunner::from_fn(|_, _| {
        Err(crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::BatchSubprocessRunError::Interrupted)
    });
    let err = execute_rust_coverage_batch_fresh_with_fake(&req, runner).unwrap_err();
    assert!(matches!(err, RustLlvmCovError::Interrupted));
    assert!(!run_root.exists());
}

#[test]
fn injected_cleanup_failure_after_success_becomes_batch_error() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let mut coverage = BTreeMap::new();
    coverage.insert(
        "pkg::bin$alpha".to_string(),
        RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    let fake = Arc::new(FakeInstanceExporter::new(coverage));
    let result = execute_fresh_batch_with_export_fn_and_cleanup(
        &req,
        &tools,
        &identity,
        &plan,
        &fake_runner(),
        Arc::new(
            move |batch_executor_request, source_root, _catalog, seed_objects| {
                fake.export_instance(batch_executor_request, source_root, &[], seed_objects)
            },
        ),
        CurrentRunCleanup::injecting(|_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        }),
    )
    .unwrap();
    let message = format!("{:?}", result.batch_error.unwrap());
    assert!(message.contains("injected cleanup failure"));
}

#[test]
fn injected_cleanup_failure_after_primary_error_preserves_primary() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    seed_durable_cache_artifacts(&req.cache_root);
    fs::create_dir_all(req.cache_root.join("entries")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            req.cache_root.join("entries"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
    }
    let mut coverage = BTreeMap::new();
    coverage.insert(
        "pkg::bin$alpha".to_string(),
        RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    let fake = Arc::new(FakeInstanceExporter::new(coverage));
    let result = execute_fresh_batch_with_export_fn_and_cleanup(
        &req,
        &tools,
        &identity,
        &plan,
        &fake_runner(),
        Arc::new(
            move |batch_executor_request, source_root, _catalog, seed_objects| {
                fake.export_instance(batch_executor_request, source_root, &[], seed_objects)
            },
        ),
        CurrentRunCleanup::injecting(|_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        }),
    )
    .unwrap();
    let message = format!("{:?}", result.batch_error.unwrap());
    assert!(message.contains("injected cleanup failure"));
    assert!(message.contains("Io"));
    assert_durable_cache_artifacts_intact(&req.cache_root);
}

#[test]
fn entries_stored_before_cleanup_failure_remain_loadable() {
    use crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint;
    use crate::rust_llvm_cov_runner::rust_cov_cache::load_rust_cov_cache_entry;

    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let mut coverage = BTreeMap::new();
    coverage.insert(
        "pkg::bin$alpha".to_string(),
        RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    let fake = Arc::new(FakeInstanceExporter::new(coverage));
    let result = execute_fresh_batch_with_export_fn_and_cleanup(
        &req,
        &tools,
        &identity,
        &plan,
        &fake_runner(),
        Arc::new(
            move |batch_executor_request, source_root, _catalog, seed_objects| {
                fake.export_instance(batch_executor_request, source_root, &[], seed_objects)
            },
        ),
        CurrentRunCleanup::injecting(|_, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        }),
    )
    .unwrap();
    assert!(result.batch_error.is_some());
    for selector in &req.logical_selectors {
        let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, selector);
        assert!(
            load_rust_cov_cache_entry(&req.cache_root, &fingerprint).is_some(),
            "stored entry for `{selector}` must remain loadable after cleanup failure"
        );
    }
}

#[test]
fn interrupted_run_is_removed_by_next_invocation() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let run_root = run_root_for(&req);
    prepare_batch_run_layout(&build_rust_coverage_batch_plan(&req).unwrap()).unwrap();
    fs::write(run_root.join("stale-marker"), b"leftover").unwrap();
    assert!(run_root.is_dir());

    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert!(result.batch_error.is_none());
    assert!(!run_root.exists());
}
