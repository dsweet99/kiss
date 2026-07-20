use std::fs;
use std::time::Duration;

use super::*;
use crate::batch_plan::RustCoverageBatchRequest;
use crate::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION, RustLlvmCovError};

#[test]
fn remove_stale_run_directories_failure_is_recoverable_on_next_run() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    let keep = cache_root.join("runs").join("run-keep");
    let stale = cache_root.join("runs").join("run-stale");
    fs::create_dir_all(&keep).unwrap();
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("marker"), b"x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o555)).unwrap();
    }
    let first = remove_stale_run_directories(&cache_root, &keep);
    assert!(first.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o755)).unwrap();
    }
    remove_stale_run_directories(&cache_root, &keep).unwrap();
    assert!(keep.is_dir());
    assert!(!stale.exists());
}

#[test]
fn remove_stale_run_directories_keeps_current_run() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    let keep = cache_root.join("runs").join("run-keep");
    let stale = cache_root.join("runs").join("run-stale");
    fs::create_dir_all(&keep).unwrap();
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("marker"), b"x").unwrap();

    remove_stale_run_directories(&cache_root, &keep).unwrap();

    assert!(keep.is_dir());
    assert!(!stale.exists());
}

#[test]
fn injectable_runner_can_replace_subprocess() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.cwd = tmp.path().to_path_buf();
    req.source_root = tmp.path().to_path_buf();
    req.generated_config = tmp.path().join("runs/run-a/nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let runner = BatchSubprocessRunner::from_fn(|_, _| {
        Ok(BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: br#"{"reason":"build-finished","success":true}"#.to_vec(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            process_residual_count: 0,
        })
    });
    let outcome = runner.run(tmp.path(), &plan).unwrap();
    assert_eq!(outcome.exit_code, Some(0));
}

#[test]
fn prepare_batch_run_layout_creates_run_and_output_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.generated_config = tmp.path().join("runs/run-a/nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let run_root = prepare_batch_run_layout(&plan).unwrap();
    assert!(run_root.ends_with("run-a"));
    assert!(plan.target_runner_output_dir.is_dir());
}

#[test]
fn run_batch_subprocess_runs_echo_with_env_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.cwd = tmp.path().to_path_buf();
    req.source_root = tmp.path().to_path_buf();
    req.generated_config = tmp.path().join("runs/run-a/nextest.toml");
    let mut plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    plan.argv = vec!["/bin/echo".to_string(), "hello".to_string()];
    plan.env.clear();
    let outcome = run_batch_subprocess(tmp.path(), &plan).unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(String::from_utf8_lossy(&outcome.stdout).trim(), "hello");
}

#[test]
fn batch_subprocess_error_converts_to_rust_llvm_cov_error() {
    let err: RustLlvmCovError = BatchSubprocessRunError::Spawn {
        program: "cargo".to_string(),
        message: "boom".to_string(),
    }
    .into();
    assert!(matches!(err, RustLlvmCovError::InvalidRequest(message) if message.contains("boom")));
}

#[test]
fn batch_subprocess_types_are_constructible() {
    let outcome = BatchSubprocessRunOutcome {
        exit_code: Some(0),
        stdout: b"ok".to_vec(),
        stderr: Vec::new(),
        duration: Duration::from_millis(1),
        process_residual_count: 0,
    };
    assert_eq!(outcome.stdout, b"ok");
}

#[test]
fn build_identity_helpers_are_executable_witnesses() {
    use crate::test_support::witness_batch_tools;

    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-a")
        .join("nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let tools = witness_batch_tools();
    let input = build_identity_input(&req, &tools);
    let _ = BuildIdentityInput {
        cache_schema: CACHE_SCHEMA_VERSION.to_string(),
        execution_policy: BATCH_EXECUTION_POLICY_VERSION.to_string(),
        tool_versions: input.tool_versions.clone(),
        source_root: input.source_root.clone(),
        cargo_args: input.cargo_args.clone(),
        env: input.env.clone(),
    };
    let _ = BuildIdentityFile {
        input: input.clone(),
        build_target_baseline_bytes: 7,
    };
    let _ = BuildIdentityPreparation {
        previous_baseline_bytes: 7,
    };

    fs::create_dir_all(&plan.build_target).unwrap();
    fs::write(plan.build_target.join("artifact"), b"12345").unwrap();
    assert_eq!(path_size_bytes(&plan.build_target).unwrap(), 5);
    assert_eq!(
        path_size_bytes(&plan.build_target.join("missing")).unwrap(),
        0
    );
    assert!(
        build_identity_path(&req.cache_root)
            .to_string_lossy()
            .ends_with("identity.json")
    );

    publish_successful_build_identity(&req, &tools, &plan, 0).unwrap();
    let prep = prepare_build_target_for_identity(&req, &tools, &plan).unwrap();
    assert_eq!(prep.previous_baseline_bytes, 5);
}

#[test]
fn prepare_build_target_for_identity_retains_external_target_when_growth_limit_exceeded() {
    use crate::test_support::witness_batch_tools;

    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-a")
        .join("nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let tools = witness_batch_tools();
    fs::create_dir_all(&plan.build_target).unwrap();
    fs::write(plan.build_target.join("artifact"), vec![0_u8; 10]).unwrap();
    publish_successful_build_identity(&req, &tools, &plan, 0).unwrap();
    fs::write(plan.build_target.join("artifact"), vec![0_u8; 20]).unwrap();
    prepare_build_target_for_identity(&req, &tools, &plan).unwrap();
    assert!(plan.build_target.exists());
}

#[test]
fn prepare_build_target_for_identity_rebuilds_cache_owned_target_when_growth_limit_exceeded() {
    use crate::test_support::witness_batch_tools;

    let tmp = tempfile::tempdir().unwrap();
    let mut req = RustCoverageBatchRequest::witness();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    req.generated_config = req
        .cache_root
        .join("runs")
        .join("run-a")
        .join("nextest.toml");
    let mut plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    plan.build_target = req.cache_root.join("build").join("target");
    let tools = witness_batch_tools();
    fs::create_dir_all(&plan.build_target).unwrap();
    fs::write(plan.build_target.join("artifact"), vec![0_u8; 10]).unwrap();
    publish_successful_build_identity(&req, &tools, &plan, 0).unwrap();
    fs::write(plan.build_target.join("artifact"), vec![0_u8; 20]).unwrap();
    prepare_build_target_for_identity(&req, &tools, &plan).unwrap();
    assert!(!plan.build_target.exists());
}
