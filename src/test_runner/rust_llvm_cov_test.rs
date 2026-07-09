use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use rust_llvm_cov_runner::RustLineCoverage;

#[test]
fn format_rust_llvm_cov_error_preserves_context_and_message() {
    let msg =
        format_rust_llvm_cov_error(RustLlvmCovError::InvalidRequest("bad selector".to_string()));

    assert!(msg.contains("rust llvm-cov failed"));
    assert!(msg.contains("bad selector"));
}

#[test]
#[should_panic(expected = "jobs must be greater than zero")]
fn run_rust_llvm_cov_selectors_rejects_zero_jobs_before_spawning() {
    let tmp = tempfile::tempdir().unwrap();

    let _ = run_rust_llvm_cov_selectors(tmp.path(), &[], &[], false, 0);
}

#[test]
fn rust_llvm_cov_request_contract_preserves_selector_and_cache_root() {
    let tmp = tempfile::tempdir().unwrap();
    let extra = vec!["--exact".to_string()];
    let req = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "tests::case",
        &extra,
        "llvm-cov 0.6.0",
        "rustc 1.88.0",
        true,
    )
    .unwrap();

    assert_eq!(req.selector, "tests::case");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.test_args, extra);
    assert!(req.force_rerun);
    assert!(req.cache_root.ends_with("rust_llvm_cov_cache"));
}

#[test]
fn bounded_rust_llvm_cov_wrapper_handles_empty_queue() {
    let results = run_rust_llvm_cov_requests_bounded(Vec::new(), 1);

    assert!(results.is_empty());
}

#[test]
fn print_rust_llvm_cov_outcome_accepts_all_status_cache_shapes() {
    for (status, cache_status) in [
        (rpytest_runner::TestStatus::Passed, RustCovCacheStatus::Hit),
        (
            rpytest_runner::TestStatus::Passed,
            RustCovCacheStatus::MissStored,
        ),
        (rpytest_runner::TestStatus::Failed, RustCovCacheStatus::Hit),
        (
            rpytest_runner::TestStatus::Failed,
            RustCovCacheStatus::MissStored,
        ),
    ] {
        print_rust_llvm_cov_outcome(&RustLlvmCovOutcome {
            selector: "tests::case".to_string(),
            status,
            exit_code: Some(i32::from(status == rpytest_runner::TestStatus::Failed)),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status,
            stdout: None,
            stderr: Some(Vec::new()),
        });
    }
}

#[test]
fn bounded_runner_assigns_and_reuses_worker_slots() {
    let tmp = tempfile::tempdir().unwrap();
    let reqs: Vec<_> = (0..5)
        .map(|index| RustLlvmCovRequest {
            selector: format!("tests::case_{index}"),
            cwd: tmp.path().to_path_buf(),
            source_root: tmp.path().to_path_buf(),
            cargo: PathBuf::from("cargo"),
            llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
            rustc_version: "rustc 1.88.0".to_string(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
            cache_root: tmp.path().join(".kiss").join("rust_llvm_cov_cache"),
            force_rerun: false,
            worker_slot: usize::MAX,
        })
        .collect();
    let seen_slots = Rc::new(RefCell::new(Vec::new()));
    let seen_slots_for_spawner = Rc::clone(&seen_slots);

    let results =
        run_rust_llvm_cov_requests_bounded_with_spawner(reqs, 2, move |index, slot, req, tx| {
            assert_eq!(req.worker_slot, slot);
            seen_slots_for_spawner.borrow_mut().push(slot);
            let outcome = RustLlvmCovOutcome {
                selector: req.selector,
                status: rpytest_runner::TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                cache_status: RustCovCacheStatus::MissStored,
                stdout: None,
                stderr: None,
            };
            tx.send((index, slot, Ok(outcome))).unwrap();
        });

    assert!(results.iter().all(Result::is_ok));
    assert_eq!(&*seen_slots.borrow(), &[0, 1, 0, 1, 0]);
}

#[test]
fn bounded_runner_removes_surplus_worker_slots() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    for slot in 0..3 {
        fs::create_dir_all(
            cache_root
                .join("workers")
                .join(format!("slot-{slot}"))
                .join("target"),
        )
        .unwrap();
    }
    let reqs: Vec<_> = (0..2)
        .map(|index| RustLlvmCovRequest {
            selector: format!("tests::case_{index}"),
            cwd: tmp.path().to_path_buf(),
            source_root: tmp.path().to_path_buf(),
            cargo: PathBuf::from("cargo"),
            llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
            rustc_version: "rustc 1.88.0".to_string(),
            cargo_args: Vec::new(),
            test_args: Vec::new(),
            env: BTreeMap::new(),
            cache_root: cache_root.clone(),
            force_rerun: false,
            worker_slot: usize::MAX,
        })
        .collect();

    let results =
        run_rust_llvm_cov_requests_bounded_with_spawner(reqs, 2, move |index, slot, req, tx| {
            let outcome = RustLlvmCovOutcome {
                selector: req.selector,
                status: rpytest_runner::TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                cache_status: RustCovCacheStatus::MissStored,
                stdout: None,
                stderr: None,
            };
            tx.send((index, slot, Ok(outcome))).unwrap();
        });

    assert!(results.iter().all(Result::is_ok));
    assert!(cache_root.join("workers").join("slot-0").exists());
    assert!(cache_root.join("workers").join("slot-1").exists());
    assert!(!cache_root.join("workers").join("slot-2").exists());
}

#[test]
fn spawn_rust_llvm_cov_job_reports_runner_errors_with_index_and_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RustLlvmCovRequest {
        selector: "tests::case".to_string(),
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        cargo: PathBuf::from("/definitely/not/cargo"),
        llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
        rustc_version: "rustc 1.88.0".to_string(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        cache_root: tmp.path().join(".kiss").join("rust_llvm_cov_cache"),
        force_rerun: true,
        worker_slot: 4,
    };
    let (tx, rx) = mpsc::channel();

    spawn_rust_llvm_cov_job(7, 4, req, tx);
    let (index, slot, result) = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    assert_eq!(index, 7);
    assert_eq!(slot, 4);
    assert!(matches!(result, Err(RustLlvmCovError::Runner(_))));
}
