use crate::batch_derived::population_derived_state_stale;
use crate::batch_executor_fresh::execute_fresh_batch_with_exporter;
use crate::batch_export::SubprocessInstanceExporter;
use crate::batch_export_tools::resolve_export_tools_from_rustc;
use crate::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, batch_identity, entry_fingerprint,
};
use crate::batch_lock::lock_batch;
use crate::batch_plan::{
    CoverageOutputMode, RustCoverageBatchPlan, RustCoverageBatchRequest,
    build_rust_coverage_batch_plan,
};
use crate::batch_result::{RustCoverageBatchCounters, RustCoverageBatchResult};
use crate::batch_run::default_batch_subprocess_runner;
use crate::worker::cleanup_legacy_worker_data_nonblocking;
use crate::{RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome};
use rpytest_runner::TestStatus;
use std::collections::BTreeMap;
use std::time::Duration;

fn default_instance_exporter(
    req: &RustCoverageBatchRequest,
    plan: &RustCoverageBatchPlan,
) -> Result<SubprocessInstanceExporter, RustLlvmCovError> {
    let export_tools = resolve_export_tools_from_rustc(std::ffi::OsStr::new("rustc"))?;
    let ignore_filename_regex =
        crate::batch_export_ignore::resolve_ignore_filename_regex(req, &plan.build_target)?;
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
        execute_fresh_batch_with_exporter(
            req,
            tools,
            identity,
            plan,
            &default_batch_subprocess_runner(),
            default_instance_exporter(req, plan)?,
        )
    })
}

/// Shared executor body used by production and regression tests.
///
/// After a fresh batch returns a result, this always routes through
/// `finalize_after_fresh_batch` so publication and selective pruning stay bound
/// to `execute_rust_coverage_batch` (including failed selective runs).
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
    crate::batch_platform::ensure_batch_platform_supported()?;
    let identity = batch_identity(req, tools)?;
    let plan = build_rust_coverage_batch_plan(req)
        .map_err(|message| RustLlvmCovError::InvalidRequest(format!("batch plan: {message}")))?;

    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
        && let Some(result) = try_all_hit_fast_path(req, tools, &identity)?
    {
        // Lock-free only when derived state already validates and no publication
        // is needed. Never publish/repair here: a TOCTOU stale flip must wait for
        // a lock-owning publisher (below or a later call).
        return Ok(with_process_reverse_query_counters(result));
    }

    let _batch_guard = lock_batch(&req.cache_root)?;
    let legacy_cleanup = cleanup_legacy_worker_data_nonblocking(&req.cache_root)?;

    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
        && let Some(result) = try_all_hit_after_lock(req, tools, &identity)?
    {
        return maybe_publish_derived_after_all_hit(req, tools, &identity, result).map(
            |mut result| {
                result.counters.legacy_cleanup_deferred = legacy_cleanup.deferred;
                with_process_reverse_query_counters(result)
            },
        );
    }
    if let Some(mut result) = try_check_aggregate_hit_after_lock(req, &identity)? {
        result.counters.legacy_cleanup_deferred = legacy_cleanup.deferred;
        return Ok(with_process_reverse_query_counters(result));
    }

    fresh(req, tools, &identity, &plan).and_then(|mut result| {
        finalize_after_fresh_batch(req, tools, &identity, legacy_cleanup.deferred, &mut result)?;
        Ok(with_process_reverse_query_counters(result))
    })
}

fn with_process_reverse_query_counters(
    mut result: RustCoverageBatchResult,
) -> RustCoverageBatchResult {
    result
        .counters
        .incorporate_process_reverse_query_counters();
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
    crate::batch_derived::maybe_prune_obsolete_selective_after_batch(req, identity, result)?;
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
    let selectors = req.population_publication_selectors.as_deref().unwrap_or(&[]);
    if let Some(publish) = crate::batch_derived::try_publish_population_derived_state_with_binaries(
        req,
        tools,
        identity,
        selectors,
        &result.test_binaries,
    )? {
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
    all_hit_batch_result(req, tools, identity)
}

fn try_all_hit_after_lock(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<RustCoverageBatchResult>, RustLlvmCovError> {
    all_hit_batch_result(req, tools, identity)
}

fn all_hit_batch_result(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<RustCoverageBatchResult>, RustLlvmCovError> {
    let Some(completed) = all_hit_outcomes(req, tools, identity)? else {
        return Ok(None);
    };
    Ok(Some(RustCoverageBatchResult {
        completed,
        batch_error: None,
        counters: RustCoverageBatchCounters {
            cache_hits: req.logical_selectors.len(),
            ..Default::default()
        },
        test_binaries: Vec::new(),
    }))
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
    let population = crate::batch_derived_index::load_current_population_state(
        &req.cache_root,
        &req.source_root,
        identity,
        Some(selectors),
    )
    .or_else(|| {
        crate::batch_derived_index::load_reusable_prior_population_state(
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
    if !population
        .entries_fingerprint
        .starts_with("check-aggregate:")
    {
        return Ok(None);
    }
    let completed = req
        .logical_selectors
        .iter()
        .map(|selector| RustLlvmCovOutcome {
            selector: selector.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::ZERO,
            coverage: crate::RustLineCoverage {
                files: BTreeMap::new(),
            },
            test_binary_ids: Vec::new(),
            cache_status: RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        })
        .collect();
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

fn all_hit_outcomes(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<Option<Vec<RustLlvmCovOutcome>>, RustLlvmCovError> {
    let mut completed = Vec::with_capacity(req.logical_selectors.len());
    for selector in &req.logical_selectors {
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
        let path = crate::rust_cov_cache::rust_cov_cache_entry_path(&req.cache_root, &fingerprint);
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(_) => return Ok(None),
        };
        let mut prefix = [0_u8; 512];
        let n = std::io::Read::read(&mut file, &mut prefix).map_err(RustLlvmCovError::Io)?;
        let status = status_from_entry_prefix(&prefix[..n]).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "invalid rust cov cache entry {}",
                path.display()
            ))
        })?;
        completed.push(RustLlvmCovOutcome {
            selector: selector.clone(),
            status,
            exit_code: match status {
                TestStatus::Passed => Some(0),
                TestStatus::Failed => Some(1),
            },
            duration: Duration::ZERO,
            coverage: Default::default(),
            test_binary_ids: Vec::new(),
            cache_status: RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        });
    }
    Ok(Some(completed))
}

fn status_from_entry_prefix(bytes: &[u8]) -> Option<TestStatus> {
    // Entries are small JSON objects; locate the status token without full deserialize.
    const FAILED: &[u8] = br#""status":"Failed""#;
    const PASSED: &[u8] = br#""status":"Passed""#;
    if bytes.windows(FAILED.len()).any(|window| window == FAILED) {
        Some(TestStatus::Failed)
    } else if bytes.windows(PASSED.len()).any(|window| window == PASSED) {
        Some(TestStatus::Passed)
    } else {
        None
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn outcome_from_entry(
    entry: crate::rust_cov_cache::RustCovCacheEntry,
    cache_status: RustCovCacheStatus,
) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: entry.selector,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        test_binary_ids: entry.test_binary_ids,
        cache_status,
        stdout: None,
        stderr: None,
    }
}

#[cfg(test)]
#[path = "batch_executor_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_executor_all_hit_lock_test.rs"]
mod all_hit_lock_tests;
