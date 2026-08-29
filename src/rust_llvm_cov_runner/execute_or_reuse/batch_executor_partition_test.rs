use super::*;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::RustCovCacheStatus;
use crate::rust_llvm_cov_runner::RustLineCoverage;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, store_batch_executor_selector,
    witness_batch_tools,
};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

fn tools() -> crate::rust_llvm_cov_runner::RustCoverageToolIdentity {
    witness_batch_tools()
}

#[test]
fn one_missing_entry_preserves_hits_and_runs_only_misses() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    store_batch_executor_selector(repo.path(), &req, "alpha");
    store_batch_executor_selector(repo.path(), &req, "beta");
    req.logical_selectors = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
    let fresh_selectors = std::sync::Mutex::new(Vec::new());
    let result = execute_rust_coverage_batch_with_fresh(&req, &tools(), |miss_req, _, _, _| {
        fresh_selectors
            .lock()
            .unwrap()
            .clone_from(&miss_req.logical_selectors);
        Ok(RustCoverageBatchResult {
            completed: miss_req
                .logical_selectors
                .iter()
                .map(|selector| crate::rust_llvm_cov_runner::RustLlvmCovOutcome {
                    selector: selector.clone(),
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    duration: Duration::from_millis(3),
                    coverage: RustLineCoverage {
                        files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
                    },
                    test_binary_ids: Vec::new(),
                    cache_status: RustCovCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                })
                .collect(),
            counters: RustCoverageBatchCounters {
                build_invocations: 1,
                ..Default::default()
            },
            batch_error: None,
            test_binaries: Vec::new(),
        })
    })
    .unwrap();
    assert_eq!(*fresh_selectors.lock().unwrap(), vec!["gamma".to_string()]);
    assert_eq!(result.counters.cache_hits, 2);
    assert_eq!(result.counters.build_invocations, 1);
    assert_eq!(
        result
            .completed
            .iter()
            .map(|outcome| outcome.selector.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "gamma"]
    );
}
