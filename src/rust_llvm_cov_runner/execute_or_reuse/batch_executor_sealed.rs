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

pub(super) fn try_sealed_all_hit(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    tools: &RustCoverageToolIdentity,
) -> Option<RustCoverageBatchResult> {
    super::batch_warm_hit_seal::try_warm_all_hit_seal(req, identity)?;
    let population =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::load_current_population_state(
            &req.cache_root,
            &req.source_root,
            identity,
            Some(&req.logical_selectors),
        )?;
    crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::current_test_binaries_match(
        &req.source_root,
        &population,
    )
    .then_some(())?;
    let entries = req
        .logical_selectors
        .iter()
        .map(|selector| {
            let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
            let entry = crate::rust_llvm_cov_runner::rust_cov_cache::load_rust_cov_cache_entry(
                &req.cache_root,
                &fingerprint,
            )?;
            (entry.generation_fingerprint == identity.generation_fingerprint
                && entry.selector == *selector
                && entry.status == TestStatus::Passed
                && !entry.coverage.files.is_empty())
            .then_some((selector.clone(), entry))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    let pairs =
        crate::rust_llvm_cov_runner::publish_derived::batch_population_durations::try_load_population_durations(
            &req.cache_root,
            &population,
        )?;
    let durations: BTreeMap<_, _> = pairs.into_iter().collect();
    let completed = req
        .logical_selectors
        .iter()
        .map(|selector| {
            let entry = entries.get(selector)?;
            Some(RustLlvmCovOutcome {
                selector: selector.clone(),
                status: TestStatus::Passed,
                exit_code: entry.exit_code,
                duration: durations.get(selector).copied()?,
                coverage: entry.coverage.clone(),
                test_binary_ids: entry.test_binary_ids.clone(),
                cache_status: RustCovCacheStatus::Hit,
                stdout: None,
                stderr: None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RustCoverageBatchResult {
        completed,
        batch_error: None,
        counters: RustCoverageBatchCounters {
            cache_hits: req.logical_selectors.len(),
            ..Default::default()
        },
        test_binaries: population.test_binaries.into_values().collect(),
    })
}

pub(super) fn write_seal_after_complete_pass(
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
        let _ = super::batch_warm_hit_seal::write_warm_all_hit_seal(req, identity);
    }
}
