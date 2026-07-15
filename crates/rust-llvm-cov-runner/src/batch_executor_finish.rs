use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::{
    RustCovCacheStatus, RustLineCoverage, RustLlvmCovError, RustLlvmCovOutcome,
    batch_aggregate::{InstanceResult, aggregate_logical_selectors},
    batch_export::{InstanceExportRequest, object_paths_for_executable},
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint},
    batch_plan::RustCoverageBatchRequest,
    batch_result::{RustCoverageBatchCounters, RustCoverageBatchResult},
    batch_shim::BatchShimMetadata,
    rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry},
};

pub(crate) struct FreshBatchFinishContext {
    pub(crate) export_started: std::time::Instant,
    pub(crate) build_target_baseline_bytes: u64,
    pub(crate) process_residual_count: usize,
}

#[cfg(test)]
impl FreshBatchFinishContext {
    pub(crate) fn witness() -> Self {
        Self {
            export_started: std::time::Instant::now(),
            build_target_baseline_bytes: 42,
            process_residual_count: 0,
        }
    }
}

pub(crate) fn build_instance_results(
    started_tests: &[crate::batch_events::BatchTestStarted],
    ignored_tests: &[crate::batch_events::BatchTestStarted],
    terminal_tests: &[crate::batch_events::BatchTestTerminal],
    shim_metadata: &[BatchShimMetadata],
    exact: bool,
    req: &RustCoverageBatchRequest,
) -> Result<Vec<InstanceResult>, RustLlvmCovError> {
    reject_missing_terminal_events(started_tests, ignored_tests, terminal_tests, exact, req)?;
    let metadata_by_id: BTreeMap<_, _> = shim_metadata
        .iter()
        .map(|item| (item.full_name.clone(), item))
        .collect();
    let mut instances = Vec::new();
    for test in terminal_tests {
        if crate::batch_events::aggregate_selectors_for_test(
            &test.full_name,
            &req.logical_selectors,
            exact,
        )
        .is_empty()
        {
            continue;
        }
        let shim = resolve_shim_metadata(&metadata_by_id, shim_metadata, &test.full_name)?;
        let exit_code = shim.exit_code.or(Some(if test.passed { 0 } else { 1 }));
        instances.push(InstanceResult {
            full_name: test.full_name.clone(),
            passed: test.passed,
            exit_code,
            duration: Duration::from_secs_f64(test.exec_time_secs),
            stdout: shim
                .stdout
                .clone()
                .or_else(|| test.stdout.as_ref().map(|value| value.as_bytes().to_vec())),
            stderr: shim
                .stderr
                .clone()
                .or_else(|| test.reason.as_ref().map(|value| value.as_bytes().to_vec())),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
        });
    }
    instances.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    Ok(instances)
}

pub(crate) fn build_instance_export_requests(
    instances: &[InstanceResult],
    shim_metadata: &[BatchShimMetadata],
    artifacts: &[crate::batch_events::BatchCompilerArtifact],
) -> Result<Vec<InstanceExportRequest>, RustLlvmCovError> {
    let metadata_by_id: BTreeMap<_, _> = shim_metadata
        .iter()
        .map(|item| (item.full_name.clone(), item))
        .collect();
    let mut requests = Vec::new();
    for instance in instances.iter().filter(|instance| instance.passed) {
        let shim = resolve_shim_metadata(&metadata_by_id, shim_metadata, &instance.full_name)?;
        let executable = shim
            .argv
            .first()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| {
                RustLlvmCovError::InvalidRequest(format!(
                    "missing test binary argv for export instance `{}`",
                    instance.full_name
                ))
            })?;
        let objects = object_paths_for_executable(artifacts, &executable);
        if objects.is_empty() {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "no instrumented objects found for export instance `{}` executable {}",
                instance.full_name,
                executable.display()
            )));
        }
        requests.push(InstanceExportRequest {
            instance_id: instance.full_name.clone(),
            profile_path: instance_profile_path(shim_metadata, &instance.full_name),
            objects,
        });
    }
    Ok(requests)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_fresh_batch_after_export(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    exact: bool,
    instances: Vec<InstanceResult>,
    exported: Vec<(String, RustLineCoverage)>,
    export_counters: crate::batch_export::ExportCounters,
    finish: FreshBatchFinishContext,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    let export_phase_ms = finish.export_started.elapsed().as_millis();
    let coverage_by_id: BTreeMap<_, _> = exported.into_iter().collect();
    let instances_with_coverage: Vec<InstanceResult> = instances
        .into_iter()
        .map(|mut instance| {
            if let Some(coverage) = coverage_by_id.get(&instance.full_name) {
                instance.coverage = coverage.clone();
            }
            instance
        })
        .collect();
    let (mut completed, agg_counters) =
        aggregate_logical_selectors(&req.logical_selectors, exact, &instances_with_coverage);
    let counters = RustCoverageBatchCounters {
        build_invocations: 1,
        test_instances: agg_counters.test_instances,
        export_jobs: export_counters.export_jobs,
        max_active_test_instances: agg_counters.test_instances.min(req.jobs),
        max_active_exports: export_counters.max_active_exports,
        unmatched_selectors: agg_counters.unmatched_selectors,
        max_objects_per_export: export_counters.max_objects_per_export,
        build_target_baseline_bytes: finish.build_target_baseline_bytes,
        export_phase_ms,
        process_residual_count: finish.process_residual_count,
        ..Default::default()
    };
    if req.population_publication_selectors.is_some() && agg_counters.unmatched_selectors > 0 {
        return Ok(RustCoverageBatchResult {
            completed: Vec::new(),
            batch_error: Some(RustLlvmCovError::InvalidRequest(format!(
                "population coverage batch did not execute {} requested Rust selector(s)",
                agg_counters.unmatched_selectors
            ))),
            counters,
        });
    }
    match store_completed_outcomes(req, tools, identity, &mut completed) {
        Ok(()) => Ok(RustCoverageBatchResult {
            completed,
            batch_error: None,
            counters,
        }),
        Err(store_err) => Ok(RustCoverageBatchResult {
            completed,
            batch_error: Some(store_err),
            counters,
        }),
    }
}

fn resolve_shim_metadata<'a>(
    metadata_by_full_name: &BTreeMap<String, &'a BatchShimMetadata>,
    shim_metadata: &'a [BatchShimMetadata],
    test_full_name: &str,
) -> Result<&'a BatchShimMetadata, RustLlvmCovError> {
    if let Some(item) = metadata_by_full_name.get(test_full_name) {
        return Ok(*item);
    }
    let Some((_, test_name)) = test_full_name.rsplit_once('$') else {
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        )));
    };
    let matches: Vec<_> = shim_metadata
        .iter()
        .filter(|item| {
            item.full_name
                .rsplit_once('$')
                .is_some_and(|(_, name)| name == test_name)
        })
        .collect();
    match matches.len() {
        0 => Err(RustLlvmCovError::InvalidRequest(format!(
            "missing target-runner metadata for test instance `{test_full_name}`"
        ))),
        1 => Ok(matches[0]),
        _ => Err(RustLlvmCovError::InvalidRequest(format!(
            "ambiguous target-runner metadata for test instance `{test_full_name}`"
        ))),
    }
}

fn reject_missing_terminal_events(
    started_tests: &[crate::batch_events::BatchTestStarted],
    ignored_tests: &[crate::batch_events::BatchTestStarted],
    terminal_tests: &[crate::batch_events::BatchTestTerminal],
    exact: bool,
    req: &RustCoverageBatchRequest,
) -> Result<(), RustLlvmCovError> {
    let terminal_names: BTreeSet<_> = terminal_tests
        .iter()
        .map(|test| test.full_name.as_str())
        .collect();
    let ignored_names: BTreeSet<_> =
        if crate::batch_events::rust_test_args_include_ignored(&req.test_args) {
            BTreeSet::new()
        } else {
            ignored_tests
                .iter()
                .map(|test| test.full_name.as_str())
                .collect()
        };
    let missing = started_tests
        .iter()
        .filter(|test| {
            !terminal_names.contains(test.full_name.as_str())
                && !ignored_names.contains(test.full_name.as_str())
                && !crate::batch_events::aggregate_selectors_for_test(
                    &test.full_name,
                    &req.logical_selectors,
                    exact,
                )
                .is_empty()
        })
        .map(|test| test.full_name.clone())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "missing terminal events for scheduled Rust test instances: {}",
        missing.join(", ")
    )))
}

fn instance_profile_path(
    shim_metadata: &[BatchShimMetadata],
    full_name: &str,
) -> std::path::PathBuf {
    let metadata_by_id: BTreeMap<_, _> = shim_metadata
        .iter()
        .map(|item| (item.full_name.clone(), item))
        .collect();
    resolve_shim_metadata(&metadata_by_id, shim_metadata, full_name)
        .map(|item| item.profile_path.clone())
        .unwrap_or_else(|_| std::path::PathBuf::from(format!("{full_name}.profraw")))
}

fn store_completed_outcomes(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    completed: &mut [RustLlvmCovOutcome],
) -> Result<(), RustLlvmCovError> {
    for outcome in completed.iter_mut() {
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, &outcome.selector);
        let cache_entry =
            RustCovCacheEntry::from_outcome(outcome, &identity.generation_fingerprint);
        match store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &cache_entry) {
            Ok(()) => outcome.cache_status = RustCovCacheStatus::MissStored,
            Err(err) => {
                outcome.cache_status = RustCovCacheStatus::FreshUnstored;
                return Err(RustLlvmCovError::Io(err));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "batch_executor_finish_test.rs"]
mod tests;
