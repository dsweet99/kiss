use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

fn rust_cov_request(tmp: &Path, selector: String) -> RustLlvmCovRequest {
    RustLlvmCovRequest {
        selector,
        cwd: tmp.to_path_buf(),
        source_root: tmp.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        llvm_cov_version: "cargo-llvm-cov 0.6.0".to_string(),
        rustc_version: "rustc 1.88.0".to_string(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        cache_root: tmp.join(".kiss").join("rust_llvm_cov_cache"),
        force_rerun: false,
        worker_slot: usize::MAX,
    }
}

#[test]
fn bounded_rust_llvm_cov_wrapper_handles_empty_queue() {
    let results = run_rust_llvm_cov_requests_bounded(Vec::new(), 1).unwrap();

    assert!(results.is_empty());
}

#[test]
fn bounded_runner_reports_missing_worker_result_at_original_index() {
    let tmp = tempfile::tempdir().unwrap();
    let reqs: Vec<_> = ["tests::reported", "tests::dropped"]
        .iter()
        .map(|selector| rust_cov_request(tmp.path(), (*selector).to_string()))
        .collect();

    let results =
        run_rust_llvm_cov_requests_bounded_with_spawner(reqs, 2, move |index, slot, req, tx| {
            if req.selector == "tests::dropped" {
                return;
            }
            tx.send((index, slot, Ok(passed_rust_llvm_cov_outcome(req.selector))))
                .unwrap();
        })
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    let err = results[1].as_ref().unwrap_err();
    let RustLlvmCovError::InvalidRequest(msg) = err else {
        panic!("expected missing worker result to be an InvalidRequest");
    };
    assert!(msg.contains("worker did not report a result"));
}

#[test]
fn bounded_runner_preserves_duplicate_selector_occurrences_in_input_order() {
    let tmp = tempfile::tempdir().unwrap();
    let selectors = ["tests::case", "tests::case", "tests::other", "tests::case"];
    let reqs: Vec<_> = selectors
        .iter()
        .map(|selector| rust_cov_request(tmp.path(), (*selector).to_string()))
        .collect();

    let results = run_rust_llvm_cov_requests_bounded_with_spawner(
        reqs,
        selectors.len(),
        move |index, slot, req, tx| {
            std::thread::spawn(move || {
                let delay_ms = u64::try_from(3usize.saturating_sub(index)).unwrap();
                std::thread::sleep(Duration::from_millis(delay_ms));
                tx.send((index, slot, Ok(passed_rust_llvm_cov_outcome(req.selector))))
                    .unwrap();
            });
        },
    )
    .unwrap();
    let ordered_selectors: Vec<_> = results
        .into_iter()
        .map(|result| result.unwrap().selector)
        .collect();

    assert_eq!(ordered_selectors, selectors);
}

#[test]
fn bounded_runner_assigns_and_reuses_worker_slots() {
    let tmp = tempfile::tempdir().unwrap();
    let reqs: Vec<_> = (0..5)
        .map(|index| rust_cov_request(tmp.path(), format!("tests::case_{index}")))
        .collect();
    let seen_slots = Rc::new(RefCell::new(Vec::new()));
    let seen_slots_for_spawner = Rc::clone(&seen_slots);

    let results =
        run_rust_llvm_cov_requests_bounded_with_spawner(reqs, 2, move |index, slot, req, tx| {
            assert_eq!(req.worker_slot, slot);
            seen_slots_for_spawner.borrow_mut().push(slot);
            tx.send((index, slot, Ok(passed_rust_llvm_cov_outcome(req.selector))))
                .unwrap();
        })
        .unwrap();

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
        .map(|index| {
            let mut req = rust_cov_request(tmp.path(), format!("tests::case_{index}"));
            req.cache_root.clone_from(&cache_root);
            req
        })
        .collect();

    let results =
        run_rust_llvm_cov_requests_bounded_with_spawner(reqs, 2, move |index, slot, req, tx| {
            tx.send((index, slot, Ok(passed_rust_llvm_cov_outcome(req.selector))))
                .unwrap();
        })
        .unwrap();

    assert!(results.iter().all(Result::is_ok));
    assert!(cache_root.join("workers").join("slot-0").exists());
    assert!(cache_root.join("workers").join("slot-1").exists());
    assert!(!cache_root.join("workers").join("slot-2").exists());
}

#[test]
fn bounded_runner_returns_cleanup_failure_before_spawning_jobs() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    fs::create_dir_all(cache_root.parent().unwrap()).unwrap();
    fs::write(&cache_root, b"not a directory").unwrap();
    let mut req = rust_cov_request(tmp.path(), "tests::case".to_string());
    req.cache_root = cache_root;
    let spawned = Rc::new(RefCell::new(0usize));
    let spawned_for_spawner = Rc::clone(&spawned);

    let err = run_rust_llvm_cov_requests_bounded_with_spawner(vec![req], 1, move |_, _, _, _| {
        *spawned_for_spawner.borrow_mut() += 1;
    })
    .unwrap_err();

    assert!(matches!(err, RustLlvmCovError::Io(_)));
    assert_eq!(*spawned.borrow(), 0);
    let msg = format_rust_llvm_cov_error(err);
    assert!(msg.contains("rust llvm-cov failed"));
}

#[test]
fn spawn_rust_llvm_cov_job_reports_runner_errors_with_index_and_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = rust_cov_request(tmp.path(), "tests::case".to_string());
    req.cargo = PathBuf::from("/definitely/not/cargo");
    req.force_rerun = true;
    req.worker_slot = 4;
    let (tx, rx) = mpsc::channel();

    spawn_rust_llvm_cov_job(7, 4, req, tx);
    let (index, slot, result) = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    assert_eq!(index, 7);
    assert_eq!(slot, 4);
    assert!(matches!(result, Err(RustLlvmCovError::Runner(_))));
}
