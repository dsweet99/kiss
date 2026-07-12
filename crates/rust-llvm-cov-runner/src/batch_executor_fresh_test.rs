use super::*;
use crate::RustCovCacheStatus;
use crate::RustLineCoverage;
use crate::RustLlvmCovError;
use crate::batch_executor_fresh::execute_fresh_batch_with_export_fn;
use crate::batch_export::{FakeInstanceExporter, write_fake_profile};
use crate::batch_plan::{RustCoverageBatchRequest, build_rust_coverage_batch_plan};
use crate::batch_result::RustCoverageBatchResult;
use crate::batch_run::{
    BatchSubprocessRunner, BuildIdentityFile, BuildIdentityPreparation, build_identity_input,
    path_size_bytes, prepare_build_target_for_identity,
};
use crate::batch_shim::BatchShimMetadata;
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn tools() -> crate::RustCoverageToolIdentity {
    witness_batch_tools()
}

fn fake_runner() -> BatchSubprocessRunner {
    BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        fs::write(plan.build_target.join("artifact"), b"target").unwrap();
        write_shim_metadata(&plan.target_runner_output_dir, "pkg::bin$alpha");
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: br#"{"reason":"compiler-artifact","executable":"/tmp/bin","filenames":["/tmp/a.o"],"fresh":false}
{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.001}
"#
            .to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    })
}

fn execute_rust_coverage_batch_fresh_with_fake(
    req: &RustCoverageBatchRequest,
    runner: BatchSubprocessRunner,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    let tools = tools();
    let identity = batch_identity(req, &tools)?;
    let plan = build_rust_coverage_batch_plan(req)
        .map_err(|message| RustLlvmCovError::InvalidRequest(format!("batch plan: {message}")))?;
    let _batch_guard = lock_batch(&req.cache_root)?;
    let mut coverage = BTreeMap::new();
    coverage.insert(
        "pkg::bin$alpha".to_string(),
        RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    let fake = Arc::new(FakeInstanceExporter::new(coverage));
    execute_fresh_batch_with_export_fn(
        req,
        &tools,
        &identity,
        &plan,
        &runner,
        Arc::new(
            move |batch_executor_request, source_root, _catalog, seed_objects| {
                fake.export_instance(batch_executor_request, source_root, &[], seed_objects)
            },
        ),
    )
}

fn write_shim_metadata(output_dir: &Path, id: &str) {
    fs::create_dir_all(output_dir).unwrap();
    let profile_path = output_dir.join(format!("{id}.profraw"));
    write_fake_profile(&profile_path, b"profile").unwrap();
    let metadata = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: id.to_string(),
        full_name: id.to_string(),
        profile_path,
        cwd: output_dir.to_path_buf(),
        argv: vec!["/tmp/bin".to_string()],
        exit_code: Some(0),
        spawn_error: None,
        shim_identity: None,
        delegated_identity: None,
        stdout: None,
        stderr: None,
    };
    fs::write(
        output_dir.join(format!("{id}.json")),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
}

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
    assert_eq!(result.counters.build_target_baseline_bytes, 6);
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
fn fresh_build_identity_removes_incompatible_target_and_sizes_nested_files() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    let tools = tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    let _ = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(path_size_bytes(&plan.build_target).unwrap(), 6);

    req.cargo_args.push("--features=changed".to_string());
    let changed_plan = build_rust_coverage_batch_plan(&req).unwrap();
    let prep = prepare_build_target_for_identity(&req, &tools, &changed_plan).unwrap();

    assert_eq!(prep.previous_baseline_bytes, 0);
    assert!(!identity.generation_fingerprint.is_empty());
    assert!(!changed_plan.build_target.exists());
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
fn fresh_batch_rejects_matching_started_test_missing_terminal_event() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let runner = BatchSubprocessRunner::from_fn(|_, plan| {
        fs::create_dir_all(&plan.build_target).unwrap();
        Ok(crate::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: br#"{"reason":"build-finished","success":true}
{"type":"test","event":"started","name":"pkg::bin$alpha"}
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
        Arc::new(|_, _, _, _| unreachable!("missing terminal should fail before export")),
    )
    .unwrap_err();

    assert!(
        matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("missing terminal events") && message.contains("pkg::bin$alpha"))
    );
}

#[test]
fn store_failure_returns_fresh_unstored_with_completed_outcome() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
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
    assert_eq!(result.completed.len(), 2);
    assert!(
        result
            .completed
            .iter()
            .any(|outcome| outcome.cache_status == RustCovCacheStatus::FreshUnstored)
    );
    assert_eq!(result.counters.build_invocations, 1);
}

#[test]
fn partial_miss_falls_through_to_fresh_execution() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");

    let result = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert!(
        result
            .completed
            .iter()
            .any(|outcome| outcome.selector == "beta")
    );
}

#[test]
fn fresh_batch_plan_keeps_repository_target_untouched() {
    let repo = batch_executor_fixture_repo();
    let repo_target = repo.path().join("target");
    fs::create_dir_all(&repo_target).unwrap();
    let marker = repo_target.join("ordinary-target-marker");
    fs::write(&marker, b"untouched").unwrap();
    let before = fs::read(&marker).unwrap();

    let req = batch_executor_request(repo.path());
    let cache_root = req.cache_root.clone();
    let plan = build_rust_coverage_batch_plan(&req).unwrap();
    assert!(
        plan.build_target.starts_with(&cache_root),
        "batch build target must live under cache root, not repository target/"
    );
    assert_ne!(
        plan.build_target,
        repo_target,
        "batch must not use repository target/"
    );
    assert_eq!(
        plan.env.get("CARGO_TARGET_DIR").map(String::as_str),
        Some(plan.build_target.to_string_lossy().as_ref())
    );

    let _ = execute_rust_coverage_batch_fresh_with_fake(&req, fake_runner()).unwrap();
    assert_eq!(fs::read(&marker).unwrap(), before);
    assert!(repo_target.is_dir());
}
