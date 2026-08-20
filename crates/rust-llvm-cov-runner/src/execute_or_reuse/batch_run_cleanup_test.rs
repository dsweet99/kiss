use std::fs;
use std::io;
use std::path::PathBuf;

use super::*;
use crate::RustLlvmCovError;
use crate::execute_or_reuse::batch_result::RustCoverageBatchResult;

#[test]
fn validate_run_directory_rejects_paths_outside_runs_root() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let outside = tmp.path().join("outside-run");
    fs::create_dir_all(&outside).unwrap();

    let err = validate_run_directory_under_cache_root(&cache_root, &outside).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(outside.is_dir());
}

#[test]
fn validate_run_directory_rejects_parent_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let escaped = cache_root.join("runs").join("..").join("escape");
    fs::create_dir_all(&escaped).unwrap();

    let err = validate_run_directory_under_cache_root(&cache_root, &escaped).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(escaped.is_dir());
}

#[test]
fn remove_current_run_directory_removes_validated_run_root() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let run_root = cache_root.join("runs").join("run-a");
    fs::create_dir_all(run_root.join("instances")).unwrap();
    fs::write(run_root.join("nextest.toml"), b"cfg").unwrap();

    remove_current_run_directory(&cache_root, &run_root).unwrap();

    assert!(!run_root.exists());
    assert!(cache_root.join("runs").is_dir());
}

#[test]
fn current_run_lifecycle_guard_cleans_up_on_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let kiss = repo.join(".kiss");
    let cache_root = kiss.join("rust_llvm_cov_cache");
    let run_root = cache_root.join("runs").join("run-a");
    let other_instances = cache_root.join("runs").join("other").join("instances");
    let kiss_profraw = kiss.join("profraw");
    let crate_dir = repo.join("crates").join("pkg");
    fs::create_dir_all(&run_root).unwrap();
    fs::create_dir_all(&other_instances).unwrap();
    fs::create_dir_all(&kiss_profraw).unwrap();
    fs::create_dir_all(&crate_dir).unwrap();
    fs::write(run_root.join("marker"), b"x").unwrap();
    fs::write(kiss_profraw.join("default_1_0_9.profraw"), b"raw").unwrap();
    fs::write(other_instances.join("intentional.profraw"), b"keep").unwrap();
    fs::write(repo.join("default_root_end_0_1.profraw"), b"root").unwrap();
    fs::write(crate_dir.join("default_crate_end_0_2.profraw"), b"crate").unwrap();

    {
        let _guard = CurrentRunLifecycleGuard::new(cache_root.clone(), run_root.clone());
        assert!(run_root.is_dir());
    }

    assert!(!run_root.exists());
    assert!(!kiss_profraw.join("default_1_0_9.profraw").exists());
    assert!(other_instances.join("intentional.profraw").exists());
    assert!(!repo.join("default_root_end_0_1.profraw").exists());
    assert!(!crate_dir.join("default_crate_end_0_2.profraw").exists());
}

#[test]
fn append_cleanup_error_preserves_primary_and_appends_cleanup() {
    let primary = RustLlvmCovError::InvalidRequest("primary".into());
    let cleanup = io::Error::new(io::ErrorKind::PermissionDenied, "cleanup denied");
    let combined = append_cleanup_error(primary, cleanup);
    assert!(
        matches!(combined, RustLlvmCovError::InvalidRequest(message) if message.contains("primary") && message.contains("cleanup denied"))
    );
}

#[test]
fn finalize_batch_result_reports_cleanup_only_failure_as_batch_error() {
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: Default::default(),
        test_binaries: Vec::new(),
    };
    let cleanup = io::Error::new(io::ErrorKind::PermissionDenied, "cleanup denied");
    let finalized = finalize_batch_result(result, None, Some(cleanup)).unwrap();
    assert!(finalized.batch_error.is_some());
}

#[test]
fn finalize_batch_result_appends_cleanup_to_primary_batch_error() {
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: Some(RustLlvmCovError::InvalidRequest("primary".into())),
        counters: Default::default(),
        test_binaries: Vec::new(),
    };
    let cleanup = io::Error::new(io::ErrorKind::PermissionDenied, "cleanup denied");
    let finalized = finalize_batch_result(result, None, Some(cleanup)).unwrap();
    let message = format!("{:?}", finalized.batch_error.unwrap());
    assert!(message.contains("primary"));
    assert!(message.contains("cleanup denied"));
}

#[test]
fn finalize_batch_result_returns_ok_when_no_cleanup_errors() {
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: Default::default(),
        test_binaries: Vec::new(),
    };

    let finalized = finalize_batch_result(result, None, None).unwrap();

    assert!(finalized.batch_error.is_none());
}

#[test]
fn sole_cleanup_error_combines_stale_and_current_cleanup_failures() {
    let stale = io::Error::new(io::ErrorKind::PermissionDenied, "stale cleanup denied");
    let current = io::Error::other("current cleanup denied");

    let err = sole_cleanup_error(Some(stale), Some(current)).expect("combined cleanup error");

    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("stale cleanup denied"));
    assert!(err.to_string().contains("current cleanup denied"));
}

#[test]
fn fresh_batch_run_scope_finish_combines_execution_and_cleanup_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let run_root = cache_root.join("runs").join("run-a");
    fs::create_dir_all(&run_root).unwrap();
    let scope = FreshBatchRunScope::begin(
        &cache_root,
        run_root,
        CurrentRunCleanup::injecting(|_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        }),
    )
    .unwrap();
    let err = scope
        .finish::<()>(Err(RustLlvmCovError::InvalidRequest("primary".into())))
        .unwrap_err();
    let message = format!("{err:?}");
    assert!(message.contains("primary"));
    assert!(message.contains("injected cleanup failure"));
}

#[test]
fn fresh_batch_run_scope_finish_reports_cleanup_error_after_success() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let run_root = cache_root.join("runs").join("run-a");
    fs::create_dir_all(&run_root).unwrap();
    let scope = FreshBatchRunScope::begin(
        &cache_root,
        run_root,
        CurrentRunCleanup::injecting(|_, _| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cleanup denied after success",
            ))
        }),
    )
    .unwrap();

    let err = scope.finish::<()>(Ok(())).unwrap_err();

    assert!(format!("{err:?}").contains("cleanup denied after success"));
}

#[test]
fn unsafe_cleanup_path_is_rejected_without_deleting() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let outside = tmp.path().join("outside-run");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("marker"), b"keep").unwrap();

    let err = remove_current_run_directory(&cache_root, &outside).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(outside.join("marker").is_file());
}

#[test]
fn begin_with_layout_sweeps_orphan_default_profraw() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let kiss = repo.join(".kiss");
    let cache_root = kiss.join("rust_llvm_cov_cache");
    let crate_dir = repo.join("crates").join("pkg");
    fs::create_dir_all(&crate_dir).unwrap();
    fs::create_dir_all(kiss.join("tmp")).unwrap();
    fs::write(repo.join("default_root_0_1.profraw"), b"root").unwrap();
    fs::write(crate_dir.join("default_crate_0_2.profraw"), b"crate").unwrap();
    fs::write(
        kiss.join("tmp").join("default_legacy_0_3.profraw"),
        b"legacy",
    )
    .unwrap();

    let mut req = crate::RustCoverageBatchRequest::witness();
    req.source_root = repo.to_path_buf();
    req.cache_root = cache_root.clone();
    req.generated_config = cache_root.join("runs").join("run-a").join("nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let scope =
        FreshBatchRunScope::begin_with_layout(&cache_root, &plan, CurrentRunCleanup::default())
            .unwrap();
    assert!(kiss.join("profraw").is_dir());
    assert!(!repo.join("default_root_0_1.profraw").exists());
    assert!(!crate_dir.join("default_crate_0_2.profraw").exists());
    assert!(!kiss.join("tmp").exists());

    fs::write(
        plan.target_runner_output_dir.join("intentional.profraw"),
        b"keep",
    )
    .unwrap();
    assert!(
        plan.target_runner_output_dir
            .join("intentional.profraw")
            .exists()
    );
    drop(scope);
}

#[test]
fn begin_with_layout_cleans_run_root_when_instances_dir_creation_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let mut req = crate::RustCoverageBatchRequest::witness();
    req.cache_root = cache_root.clone();
    req.generated_config = cache_root.join("runs").join("run-a").join("nextest.toml");
    let plan = crate::build_rust_coverage_batch_plan(&req).unwrap();
    let run_root = cache_root.join("runs").join("run-a");
    let instances = run_root.join("instances");
    fs::create_dir_all(&run_root).unwrap();
    fs::write(&instances, b"not-a-directory").unwrap();
    let result =
        FreshBatchRunScope::begin_with_layout(&cache_root, &plan, CurrentRunCleanup::default());
    assert!(
        result.is_err(),
        "instances path should not be creatable as directory"
    );
    assert!(!run_root.exists());
}

#[test]
fn validate_run_directory_rejects_symlinked_runs_root() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    fs::create_dir_all(outside.join("run-a")).unwrap();
    fs::write(outside.join("run-a").join("marker"), b"keep").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&outside, cache_root.join("runs")).unwrap();
    }
    #[cfg(not(unix))]
    {
        return;
    }
    let run_root = cache_root.join("runs").join("run-a");
    let err = remove_current_run_directory(&cache_root, &run_root).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    assert!(outside.join("run-a").join("marker").is_file());
}

#[test]
fn batch_run_cleanup_types_are_constructible() {
    let _ = CurrentRunCleanup::default_cleanup();
    let _path = PathBuf::from("/tmp/run");
}
