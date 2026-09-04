use super::super::batch_executor_prepare::{banned_timeout_outcome, selector_timeout_is_ban};
use super::with_process_reverse_query_counters;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_result::{
    RustCoverageBatchCounters, RustCoverageBatchResult,
};
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, batch_identity,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::{CoverageOutputMode, RustCoverageBatchRequest};
use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome};
use std::collections::BTreeMap;

pub(super) fn resolve_batch_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Result<RustCoverageBatchIdentity, RustLlvmCovError> {
    if crate::rust_llvm_cov_runner::identity_memo_is_populated() {
        return batch_identity(req, tools).map_err(RustLlvmCovError::Io);
    }
    crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-identity",
        || batch_identity(req, tools).map_err(RustLlvmCovError::Io),
    )
}

pub(super) fn banned_timeout_batch_result(
    req: &RustCoverageBatchRequest,
) -> Option<RustCoverageBatchResult> {
    if req.logical_selectors.is_empty()
        || !req
            .logical_selectors
            .iter()
            .all(|selector| selector_timeout_is_ban(req, selector))
    {
        return None;
    }
    Some(with_process_reverse_query_counters(
        RustCoverageBatchResult {
            completed: req
                .logical_selectors
                .iter()
                .map(|selector| banned_timeout_outcome(selector))
                .collect(),
            batch_error: None,
            counters: RustCoverageBatchCounters::default(),
            test_binaries: Vec::new(),
        },
    ))
}

pub(super) fn try_reuse_before_lock(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<RustCoverageBatchResult>, RustLlvmCovError> {
    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
        && let Some(result) =
            crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_sealed::try_sealed_all_hit(
                req, identity, tools,
            )
    {
        return Ok(Some(with_process_reverse_query_counters(result)));
    }
    if !req.force_rerun
        && let Some(result) = try_check_aggregate_hit(req, identity)?
    {
        return Ok(Some(with_process_reverse_query_counters(result)));
    }
    Ok(None)
}

pub(super) fn try_check_aggregate_hit(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<RustCoverageBatchResult>, RustLlvmCovError> {
    if !matches!(
        req.coverage_output_mode,
        CoverageOutputMode::CheckAggregate { .. }
    ) {
        return Ok(None);
    }
    let Some(selectors) = req.population_publication_selectors.as_deref() else {
        return Ok(None);
    };
    let population = crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::
        load_current_population_state(
        &req.cache_root,
        &req.source_root,
        identity,
        Some(selectors),
    );
    let Some(population) = population else {
        return Ok(None);
    };
    let Some(completed) = check_aggregate_hit_completed(req, &population) else {
        return Ok(None);
    };
    Ok(Some(RustCoverageBatchResult {
        completed,
        batch_error: None,
        counters: RustCoverageBatchCounters {
            cache_hits: req.logical_selectors.len(),
            ..Default::default()
        },
        test_binaries: population.test_binaries.into_values().collect(),
    }))
}

fn check_aggregate_hit_completed(
    req: &RustCoverageBatchRequest,
    population: &crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::RustPopulationState,
) -> Option<Vec<RustLlvmCovOutcome>> {
    if !population
        .entries_fingerprint
        .starts_with("check-aggregate:")
    {
        return None;
    }
    crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::current_test_binaries_match(
        &req.source_root,
        population,
    )
    .then_some(())?;
    crate::rust_llvm_cov_runner::publish_derived::batch_population_durations::
        population_entries_all_pass(&req.cache_root, population)
        .then_some(())?;
    population_duration_hit_completed(req, population)
}

fn population_duration_hit_completed(
    req: &RustCoverageBatchRequest,
    population: &crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::RustPopulationState,
) -> Option<Vec<RustLlvmCovOutcome>> {
    let pairs = crate::rust_llvm_cov_runner::publish_derived::batch_population_durations::try_load_population_durations(
        &req.cache_root,
        population,
    )?;
    let duration_by_selector: BTreeMap<_, _> = pairs.into_iter().collect();
    req.logical_selectors
        .iter()
        .map(|selector| {
            let duration = duration_by_selector.get(selector).copied()?;
            Some(RustLlvmCovOutcome {
                selector: selector.clone(),
                status: TestStatus::Passed,
                exit_code: Some(0),
                duration,
                coverage: crate::rust_llvm_cov_runner::RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: Vec::new(),
                cache_status: RustCovCacheStatus::Hit,
                stdout: None,
                stderr: None,
            })
        })
        .collect()
}
