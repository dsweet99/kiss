use std::collections::BTreeMap;

use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_result::{
    RustCoverageBatchCounters, RustCoverageBatchResult,
};
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLlvmCovOutcome};

pub(super) fn publish_durations_after_complete_pass(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    result: &RustCoverageBatchResult,
) {
    if result.batch_error.is_some() {
        return;
    }
    let all_passed = req.logical_selectors.iter().all(|selector| {
        result
            .completed
            .iter()
            .any(|outcome| outcome.selector == *selector && outcome.status == TestStatus::Passed)
    });
    if all_passed {
        publish_durations_from_completed(req, identity, result);
    }
}

fn publish_durations_from_completed(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    result: &RustCoverageBatchResult,
) {
    let Some(population) = crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::
        load_current_population_state(
        &req.cache_root,
        &req.source_root,
        identity,
        Some(&req.logical_selectors),
    ) else {
        return;
    };
    if crate::rust_llvm_cov_runner::publish_derived::batch_population_durations::try_load_population_durations(
        &req.cache_root,
        &population,
    )
    .is_some()
    {
        return;
    }
    let pairs: Vec<(String, std::time::Duration)> = result
        .completed
        .iter()
        .map(|outcome| (outcome.selector.clone(), outcome.duration))
        .collect();
    if pairs.len() != population.selectors.len() {
        return;
    }
    if crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::read_entry_state(
        &req.cache_root,
    )
    .is_none()
    {
        let _ = crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::publish_next_entry_state(
            &req.cache_root,
            &population.generation_fingerprint,
            &population.entries_fingerprint,
        );
    }
    let _ = crate::rust_llvm_cov_runner::publish_derived::batch_population_durations::
        write_population_durations_for_warm(
        &req.cache_root,
        &population,
        &pairs,
    );
}
