use super::*;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::RustCovCacheStatus;
use crate::rust_llvm_cov_runner::RustLineCoverage;
use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::batch_identity;
use crate::rust_llvm_cov_runner::rust_cov_cache::RustCovCacheEntry;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, witness_batch_tools,
};
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
