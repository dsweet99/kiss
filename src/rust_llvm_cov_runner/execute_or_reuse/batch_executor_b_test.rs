use super::*;
use crate::rust_llvm_cov_runner::RustCovCacheStatus;
use crate::rust_llvm_cov_runner::RustLineCoverage;
use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity;
use crate::rust_llvm_cov_runner::rust_cov_cache::RustCovCacheEntry;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, witness_batch_tools,
};
use crate::rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[test]
fn apply_population_derived_publication_skips_errors_and_missing_selectors() {
    let repo = batch_executor_fixture_repo();
    let req = batch_executor_request(repo.path());
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut errored = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: Some(RustLlvmCovError::InvalidRequest("failed".to_string())),
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };
    apply_population_derived_publication(&req, &tools, &identity, &mut errored).unwrap();
    assert!(!errored.counters.derived_state_published);

    let mut no_population = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };
    apply_population_derived_publication(&req, &tools, &identity, &mut no_population).unwrap();
    assert!(!no_population.counters.derived_state_published);
}

#[test]
fn outcome_from_entry_replays_cache_entry_without_output() {
    let entry = RustCovCacheEntry::from_outcome(
        &crate::rust_llvm_cov_runner::RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
            test_binary_ids: vec!["bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: Some(b"stdout".to_vec()),
            stderr: Some(b"stderr".to_vec()),
        },
        "generation",
    );

    let outcome = outcome_from_entry(entry, RustCovCacheStatus::Hit);

    assert_eq!(outcome.selector, "alpha");
    assert_eq!(outcome.cache_status, RustCovCacheStatus::Hit);
    assert_eq!(outcome.stdout, None);
    assert_eq!(outcome.stderr, None);
}
