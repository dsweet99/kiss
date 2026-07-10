use std::fs;
use std::time::Duration;

use rpytest_runner::TestStatus;

use super::{CargoLlvmCovRunOutcome, RustLlvmCovError, finalize};
use crate::test_support::write_demo_crate_source;

#[test]
fn rust_llvm_cov_finalization_only_failure_has_no_synthetic_primary() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let mut req = super::rust_cov_sample_request(tmp.path());
    req.cache_root = tmp.path().join(".rust_llvm_cov_cache");
    fs::create_dir_all(&req.cache_root).unwrap();
    fs::write(req.cache_root.join("entries"), b"not a directory").unwrap();
    let artifact = finalize::rust_cov_artifact_path(&req.cache_root, "passed-store-fails");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(
        &artifact,
        format!(
            r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,1,true,true,false]]}}]}}]}}"#,
            tmp.path().join("src").join("lib.rs").display()
        ),
    )
    .unwrap();

    let err = finalize::finalize_run(
        &req,
        "passed-store-fails",
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
            artifact_path: artifact.clone(),
        }),
    )
    .unwrap_err();

    assert!(matches!(err, RustLlvmCovError::Finalization(errors) if errors.len() == 1));
    assert!(artifact.exists());
}

#[test]
fn rust_llvm_cov_nonzero_cache_store_failure_preserves_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let mut req = super::rust_cov_sample_request(tmp.path());
    req.cache_root = tmp.path().join(".rust_llvm_cov_cache");
    fs::create_dir_all(&req.cache_root).unwrap();
    fs::write(req.cache_root.join("entries"), b"not a directory").unwrap();
    let artifact = finalize::rust_cov_artifact_path(&req.cache_root, "failed-store-fails");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(&artifact, b"diagnostic raw artifact").unwrap();

    let err = finalize::finalize_run(
        &req,
        "failed-store-fails",
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector.clone(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration: Duration::from_millis(1),
            stdout: b"failed".to_vec(),
            stderr: Vec::new(),
            artifact_path: artifact.clone(),
        }),
    )
    .unwrap_err();

    assert!(matches!(err, RustLlvmCovError::Finalization(errors) if errors.len() == 1));
    assert!(artifact.exists());
}
