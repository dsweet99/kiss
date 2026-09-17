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
    crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_prepare::reset_prepare_count();
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
    assert_eq!(
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_prepare::prepare_count(),
        1
    );
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

#[test]
fn one_missing_among_one_thousand_preserves_hits() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    let tools = tools();
    let identity =
        crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let selectors: Vec<String> = (0..1000).map(|i| format!("sel{i:04}")).collect();
    for selector in selectors.iter().take(999) {
        store_passed_entry(&req, &tools, &identity, selector);
    }
    crate::rust_llvm_cov_runner::write_ordinary_source_snapshot(
        &req.cache_root,
        &req.source_root,
        &identity,
    )
    .unwrap();
    req.logical_selectors = selectors.clone();
    let missing = selectors[999].clone();
    let fresh_selectors = std::sync::Mutex::new(Vec::new());
    let result = execute_rust_coverage_batch_with_fresh(&req, &tools, |miss_req, _, _, _| {
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
                export_jobs: 1,
                ..Default::default()
            },
            batch_error: None,
            test_binaries: Vec::new(),
        })
    })
    .unwrap();
    assert_eq!(*fresh_selectors.lock().unwrap(), vec![missing.clone()]);
    assert_eq!(result.counters.cache_hits, 999);
    assert_eq!(result.counters.build_invocations, 1);
    assert_eq!(result.counters.export_jobs, 1);
    assert_eq!(result.completed.len(), 1000);
    assert_eq!(result.completed[999].selector, missing);
}

fn store_passed_entry(
    req: &crate::rust_llvm_cov_runner::RustCoverageBatchRequest,
    tools: &crate::rust_llvm_cov_runner::RustCoverageToolIdentity,
    identity: &crate::rust_llvm_cov_runner::RustCoverageBatchIdentity,
    selector: &str,
) {
    use crate::rust_llvm_cov_runner::plan::batch_fingerprint::entry_fingerprint;
    use crate::rust_llvm_cov_runner::rust_cov_cache::{
        RustCovCacheEntry, store_rust_cov_cache_entry,
    };

    let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
    let entry = RustCovCacheEntry::from_outcome(
        &crate::rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(7),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
}
