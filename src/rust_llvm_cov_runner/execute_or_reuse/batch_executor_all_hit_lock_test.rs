use super::*;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::lock_batch;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use std::sync::mpsc;
use std::time::Duration;

fn tools() -> crate::rust_llvm_cov_runner::RustCoverageToolIdentity {
    witness_batch_tools()
}

#[test]
fn all_hit_fast_path_skips_publish_when_derived_already_valid() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    let tools = tools();
    let first = execute_rust_coverage_batch(&population_req, &tools).unwrap();
    assert!(first.counters.derived_state_published);
    let second = execute_rust_coverage_batch(&population_req, &tools).unwrap();
    assert_eq!(second.counters.cache_hits, 2);
    assert!(!second.counters.derived_state_published);
    assert!(!second.counters.derived_repair);
    assert_eq!(second.counters.build_invocations, 0);
}

#[test]
fn all_hit_derived_repair_blocks_on_held_batch_lock() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    let mut population_req = req.clone();
    population_req.population_publication_selectors =
        Some(vec!["alpha".to_string(), "beta".to_string()]);
    let cache_root = population_req.cache_root.clone();
    let (lock_held_tx, lock_held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = lock_batch(&cache_root).unwrap();
        lock_held_tx.send(()).expect("signal lock held");
        release_rx.recv().expect("wait for release");
    });
    lock_held_rx.recv().expect("holder must take batch.lock");
    let tools = tools();
    let repairer = std::thread::spawn(move || execute_rust_coverage_batch(&population_req, &tools));
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        !repairer.is_finished(),
        "all-hit derived repair must block on batch.lock; finished early implies lock-free publish"
    );
    release_tx.send(()).expect("release lock");
    let result = repairer.join().expect("repairer").unwrap();
    holder.join().unwrap();
    assert!(result.counters.derived_state_published);
    assert!(result.counters.derived_repair);
}
