use super::*;
use crate::RustCovCacheStatus;
use crate::batch_lock::lock_batch;
use crate::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use std::fs;
use std::sync::mpsc;
use std::time::Duration;

fn tools() -> crate::RustCoverageToolIdentity {
    witness_batch_tools()
}

#[test]
fn all_hit_batch_returns_without_batch_lock_or_spawn() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");

    let result = execute_rust_coverage_batch(&req, &tools()).unwrap();
    assert_eq!(result.completed.len(), 2);
    assert!(result.batch_error.is_none());
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 0);
    assert!(
        result
            .completed
            .iter()
            .all(|outcome| { outcome.cache_status == RustCovCacheStatus::Hit })
    );
}

#[test]
fn all_hit_derived_repair_acquires_batch_lock_without_spawn() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);

    let result = execute_rust_coverage_batch(&population_req, &tools()).unwrap();
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 0);
    assert!(result.counters.derived_state_published);
    assert!(result.counters.derived_repair);
}

#[test]
fn all_hit_derived_repair_reports_deferred_legacy_cleanup() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    fs::create_dir_all(population_req.cache_root.join("workers").join("slot-0")).unwrap();
    let _slot_guard = crate::worker::lock_worker_for_test(&population_req.cache_root, 0).unwrap();

    let result = execute_rust_coverage_batch(&population_req, &tools()).unwrap();

    assert_eq!(result.counters.cache_hits, 2);
    assert!(result.counters.legacy_cleanup_deferred);
}

#[test]
fn post_lock_recheck_becomes_hit_after_another_process_stores_entry() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let mut alpha_req = req.clone();
    alpha_req.logical_selectors = vec!["alpha".to_string()];
    let cache_root = req.cache_root.clone();
    let publisher_req = req.clone();

    let (lock_acquired_tx, lock_acquired_rx) = mpsc::channel();
    let publisher = std::thread::spawn(move || {
        let _guard = lock_batch(&cache_root).unwrap();
        lock_acquired_tx.send(()).expect("publisher lock signal");
        std::thread::sleep(Duration::from_millis(100));
        store_batch_executor_selector(publisher_req.cwd.as_path(), &publisher_req, "alpha");
    });

    lock_acquired_rx
        .recv()
        .expect("publisher must acquire batch lock before recheck");
    let result = execute_rust_coverage_batch(&alpha_req, &tools()).unwrap();
    publisher.join().unwrap();

    assert_eq!(result.completed.len(), 1);
    assert_eq!(result.completed[0].cache_status, RustCovCacheStatus::Hit);
    assert_eq!(result.counters.cache_hits, 1);
    assert_eq!(result.counters.build_invocations, 0);
}
