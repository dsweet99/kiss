use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use rpytest_runner::TestStatus;

use super::{
    CargoLlvmCovRunError, CargoLlvmCovRunOutcome, CargoLlvmCovRunner, RustCovCacheStatus,
    RustLlvmCov, RustLlvmCovError, rust_cov_cache, rust_cov_sample_request, worker,
};
use crate::test_support::{llvm_cov_json_for_file, write_demo_crate_source};

#[test]
fn selector_lock_failure_returns_error_without_running_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let req = rust_cov_sample_request(tmp.path());
    let fingerprint = rust_cov_cache::rust_cov_fingerprint(&req).unwrap();
    worker::lock_failure_injection::inject_selector_lock_failure(&req.cache_root, &fingerprint);
    let calls = Rc::new(Cell::new(0));
    let cov = RustLlvmCov::new(counting_runner(Rc::clone(&calls)));

    let err = cov.run_or_reuse(req).unwrap_err();

    assert_injected_lock_error(err);
    assert_eq!(calls.get(), 0);
}

#[test]
fn legacy_cleanup_lock_failure_returns_error_without_running_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let req = rust_cov_sample_request(tmp.path());
    worker::lock_failure_injection::inject_legacy_cleanup_lock_failure(&req.cache_root);
    let calls = Rc::new(Cell::new(0));
    let cov = RustLlvmCov::new(counting_runner(Rc::clone(&calls)));

    let err = cov.run_or_reuse(req).unwrap_err();

    assert_injected_lock_error(err);
    assert_eq!(calls.get(), 0);
}

#[test]
fn worker_lock_failure_returns_error_without_running_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let req = rust_cov_sample_request(tmp.path());
    worker::lock_failure_injection::inject_worker_lock_failure(&req.cache_root, req.worker_slot);
    let calls = Rc::new(Cell::new(0));
    let cov = RustLlvmCov::new(counting_runner(Rc::clone(&calls)));

    let err = cov.run_or_reuse(req).unwrap_err();

    assert_injected_lock_error(err);
    assert_eq!(calls.get(), 0);
}

#[test]
fn early_runner_error_releases_locks_for_later_success() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let lib = tmp.path().join("src").join("lib.rs");
    let calls = Rc::new(Cell::new(0));
    let runner_calls = Rc::clone(&calls);
    let runner = CargoLlvmCovRunner::from_fn(move |req| {
        runner_calls.set(runner_calls.get() + 1);
        if runner_calls.get() == 1 {
            return Err(CargoLlvmCovRunError::Spawn {
                program: PathBuf::from("cargo"),
                message: "first run fails".to_string(),
            });
        }
        fs::create_dir_all(req.artifact_path.parent().unwrap()).unwrap();
        fs::write(&req.artifact_path, llvm_cov_json_for_file(&lib)).unwrap();
        Ok(passed_run(req))
    });
    let cov = RustLlvmCov::new(runner);
    let req = rust_cov_sample_request(tmp.path());

    let first = cov.run_or_reuse(req.clone()).unwrap_err();
    let second = cov.run_or_reuse(req).unwrap();

    assert!(matches!(first, RustLlvmCovError::Runner(_)));
    assert_eq!(second.cache_status, RustCovCacheStatus::MissStored);
    assert_eq!(calls.get(), 2);
}

fn counting_runner(calls: Rc<Cell<usize>>) -> CargoLlvmCovRunner {
    CargoLlvmCovRunner::from_fn(move |_| {
        calls.set(calls.get() + 1);
        Err(CargoLlvmCovRunError::Spawn {
            program: PathBuf::from("cargo"),
            message: "should not run".to_string(),
        })
    })
}

fn assert_injected_lock_error(err: RustLlvmCovError) {
    match err {
        RustLlvmCovError::Io(err) => {
            assert!(err.to_string().contains("injected lock failure"));
        }
        other => panic!("expected injected lock io error, got {other:?}"),
    }
}

fn passed_run(req: super::CargoLlvmCovRunRequest) -> CargoLlvmCovRunOutcome {
    CargoLlvmCovRunOutcome {
        selector: req.selector,
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
        artifact_path: req.artifact_path,
    }
}
