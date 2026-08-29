use super::batch_executor_prepare::{
    banned_timeout_outcome, merge_prepared, outcome_from_entry, prepare_rust_batch,
    selector_timeout_is_ban,
};
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_fresh::execute_fresh_batch_with_exporter;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::SubprocessInstanceExporter;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::resolve_export_tools_from_rustc;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::lock_batch;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_result::{
    RustCoverageBatchCounters, RustCoverageBatchResult,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::default_batch_subprocess_runner;
use crate::rust_llvm_cov_runner::execute_or_reuse::worker::cleanup_legacy_worker_data_nonblocking;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, batch_identity,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::{
    CoverageOutputMode, RustCoverageBatchPlan, RustCoverageBatchRequest,
    build_rust_coverage_batch_plan,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_derived::population_derived_state_stale;
use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome};
use std::collections::BTreeMap;
use std::time::Duration;

fn default_instance_exporter(
    req: &RustCoverageBatchRequest,
    plan: &RustCoverageBatchPlan,
) -> Result<SubprocessInstanceExporter, RustLlvmCovError> {
    let export_tools = resolve_export_tools_from_rustc(std::ffi::OsStr::new("rustc"))?;
    let ignore_filename_regex =
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_ignore::resolve_ignore_filename_regex(
            req,
            &plan.build_target,
        )?;
    Ok(SubprocessInstanceExporter::new(
        export_tools,
        ignore_filename_regex,
    ))
}

pub fn execute_rust_coverage_batch(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    execute_rust_coverage_batch_with_fresh(req, tools, |req, tools, identity, plan| {
        let exporter = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
            "export-prep",
            || default_instance_exporter(req, plan),
        )?;
        execute_fresh_batch_with_exporter(
            req,
            tools,
            identity,
            plan,
            &default_batch_subprocess_runner(),
            exporter,
        )
    })
}

pub(crate) fn execute_rust_coverage_batch_with_fresh<F>(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    fresh: F,
) -> Result<RustCoverageBatchResult, RustLlvmCovError>
where
    F: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolIdentity,
        &RustCoverageBatchIdentity,
        &RustCoverageBatchPlan,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError>,
{
    crate::rust_llvm_cov_runner::plan::batch_platform::ensure_batch_platform_supported()?;
    let identity = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-identity",
        || batch_identity(req, tools).map_err(RustLlvmCovError::Io),
    )?;

    if !req.logical_selectors.is_empty()
        && req
            .logical_selectors
            .iter()
            .all(|selector| selector_timeout_is_ban(req, selector))
    {
        return Ok(with_process_reverse_query_counters(
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
        ));
    }

    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
        && let Some(result) = try_all_hit_fast_path(req, tools, &identity)?
    {
        return Ok(with_process_reverse_query_counters(result));
    }

    let _batch_guard = lock_batch(&req.cache_root)?;
    let legacy_cleanup = cleanup_legacy_worker_data_nonblocking(&req.cache_root)?;

    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
    {
        return run_locked_selector_entries(req, tools, &identity, fresh, legacy_cleanup.deferred);
    }

    let plan = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-plan",
        || {
            build_rust_coverage_batch_plan(req).map_err(|message| {
                RustLlvmCovError::InvalidRequest(format!("batch plan: {message}"))
            })
        },
    )?;
    if let Some(mut result) = try_check_aggregate_hit_after_lock(req, &identity)? {
        result.counters.legacy_cleanup_deferred = legacy_cleanup.deferred;
        return Ok(with_process_reverse_query_counters(result));
    }

    fresh(req, tools, &identity, &plan).and_then(|mut result| {
        finalize_after_fresh_batch(req, tools, &identity, legacy_cleanup.deferred, &mut result)?;
        Ok(with_process_reverse_query_counters(result))
    })
}

fn run_locked_selector_entries<F>(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    fresh: F,
    deferred: bool,
) -> Result<RustCoverageBatchResult, RustLlvmCovError>
where
    F: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolIdentity,
        &RustCoverageBatchIdentity,
        &RustCoverageBatchPlan,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError>,
{
    let prepared = prepare_rust_batch(req, tools, identity)?;
    if prepared.misses_empty() {
        return maybe_publish_derived_after_all_hit(
            req,
            tools,
            identity,
            prepared.hit_result(&req.logical_selectors),
        )
        .map(|mut result| {
            result.counters.legacy_cleanup_deferred = deferred;
            with_process_reverse_query_counters(result)
        });
    }
    let mut miss_req = req.clone();
    miss_req.logical_selectors = prepared.misses.clone();
    let miss_plan = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-plan",
        || {
            build_rust_coverage_batch_plan(&miss_req).map_err(|message| {
                RustLlvmCovError::InvalidRequest(format!("batch plan: {message}"))
            })
        },
    )?;
    fresh(&miss_req, tools, identity, &miss_plan).and_then(|fresh_result| {
        let mut result = merge_prepared(&prepared, &req.logical_selectors, Some(fresh_result));
        finalize_after_fresh_batch(req, tools, identity, deferred, &mut result)?;
        Ok(with_process_reverse_query_counters(result))
    })
}

fn with_process_reverse_query_counters(
    mut result: RustCoverageBatchResult,
) -> RustCoverageBatchResult {
    result.counters.incorporate_process_reverse_query_counters();
    result
}

fn finalize_after_fresh_batch(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    legacy_cleanup_deferred: bool,
    result: &mut RustCoverageBatchResult,
) -> Result<(), RustLlvmCovError> {
    if !matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) {
        result.counters.legacy_cleanup_deferred = legacy_cleanup_deferred;
        return Ok(());
    }
    apply_population_derived_publication(req, tools, identity, result)?;
    crate::rust_llvm_cov_runner::publish_derived::batch_derived::maybe_prune_obsolete_selective_after_batch(
        req, identity, result,
    )?;
    result.counters.legacy_cleanup_deferred = legacy_cleanup_deferred;
    Ok(())
}

fn maybe_publish_derived_after_all_hit(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    mut result: RustCoverageBatchResult,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    apply_population_derived_publication(req, tools, identity, &mut result)?;
    Ok(result)
}

fn apply_population_derived_publication(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    result: &mut RustCoverageBatchResult,
) -> Result<(), RustLlvmCovError> {
    if result.batch_error.is_some() {
        return Ok(());
    }
    let selectors = req
        .population_publication_selectors
        .as_deref()
        .unwrap_or(&[]);
    if let Some(publish) =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived::try_publish_population_derived_state_with_binaries(
            req,
            tools,
            identity,
            selectors,
            &result.test_binaries,
        )?
    {
        result.counters.derived_state_published = true;
        result.counters.derived_repair = publish.derived_repair;
        result.counters.entry_generation_count = publish.entry_generation_count;
        result.counters.current_index_generation = publish.current_index_generation;
        result.counters.cache_pruned_entries = publish.cache_pruned_entries;
        result.counters.reverse_published = publish.reverse_published;
        result.counters.reverse_snapshots_reclaimed = publish.reverse_snapshots_reclaimed;
    }
    Ok(())
}

fn try_all_hit_fast_path(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<RustCoverageBatchResult>, RustLlvmCovError> {
    if population_derived_state_stale(req, tools, identity)? {
        return Ok(None);
    }
    let prepared = prepare_rust_batch(req, tools, identity)?;
    if !prepared.misses_empty() {
        return Ok(None);
    }
    Ok(Some(prepared.hit_result(&req.logical_selectors)))
}

fn try_check_aggregate_hit_after_lock(
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
    let population = crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::load_current_population_state(
        &req.cache_root,
        &req.source_root,
        identity,
        Some(selectors),
    )
    .or_else(|| {
        crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::load_reusable_prior_population_state(
            &req.cache_root,
            &req.source_root,
            Some(selectors),
            &identity.selection_context_fingerprint,
        )
        .filter(|population| population.ordinary_source_digests == identity.ordinary_source_digests)
    });
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

#[cfg(test)]
#[path = "batch_executor_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_executor_b_test.rs"]
mod tests_b;

#[cfg(test)]
#[path = "batch_executor_all_hit_lock_test.rs"]
mod all_hit_lock_tests;

#[cfg(test)]
#[path = "batch_executor_partition_test.rs"]
mod partition_tests;
