use std::path::PathBuf;

use rpytest_runner::TestStatus;

use super::cargo_runner;
use super::subprocess_cargo_llvm_cov_runner;

fn request(cargo: PathBuf) -> cargo_runner::CargoLlvmCovRunRequest {
    let tmp = tempfile::tempdir().unwrap();
    cargo_runner::CargoLlvmCovRunRequest::witness(
        cargo,
        tmp.path().to_path_buf(),
        tmp.path().join("coverage.json"),
    )
}

#[test]
fn cargo_runner_rejects_empty_selectors_before_spawning() {
    let runner = cargo_runner::CargoLlvmCovRunner::from_fn(|_| {
        panic!("runner closure should not be called for invalid selector")
    });
    let mut req = request(PathBuf::from("cargo"));
    req.selector.clear();

    let err = runner.run_one(req).unwrap_err();

    assert!(matches!(
        err,
        cargo_runner::CargoLlvmCovRunError::InvalidRequest(message) if message.contains("selector")
    ));
}

#[test]
fn cargo_runner_reports_spawn_errors_from_subprocess_boundary() {
    let req = request(PathBuf::from("/definitely/not/a/cargo"));
    let err = cargo_runner::run_subprocess(req).unwrap_err();

    assert!(matches!(
        err,
        cargo_runner::CargoLlvmCovRunError::Spawn { .. }
    ));
}

#[test]
fn cargo_runner_factory_uses_the_subprocess_boundary() {
    let runner = subprocess_cargo_llvm_cov_runner();
    let req = request(PathBuf::from("/definitely/not/a/cargo"));

    let err = runner.run_one(req).unwrap_err();

    assert!(matches!(
        err,
        cargo_runner::CargoLlvmCovRunError::Spawn { .. }
    ));
}

#[test]
fn cargo_runner_outcome_preserves_process_result_fields() {
    let outcome = cargo_runner::CargoLlvmCovRunOutcome::witness();

    assert_eq!(outcome.selector, "smoke_sub");
    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.exit_code, Some(101));
    assert_eq!(outcome.stdout, b"out");
    assert_eq!(outcome.stderr, b"err");
}
