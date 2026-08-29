use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) use crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_finish_export::FreshCheckAggregateExport;
use crate::rust_llvm_cov_runner::{
    RustLineCoverage, RustLlvmCovError, RustTestBinaryIdentity,
    batch_aggregate::InstanceResult,
    batch_executor_finish_bans::{aggregate_with_zero_limit_bans, unmatched_selectors_batch_error},
    batch_executor_finish_store::store_completed_outcomes,
    batch_export::{InstanceExportRequest, object_paths_for_executable},
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity},
    batch_plan::{CheckAggregateRepairPublication, RustCoverageBatchRequest},
    batch_result::{RustCoverageBatchCounters, RustCoverageBatchResult},
    batch_shim::BatchShimMetadata,
    batch_shim_lookup::resolve_shim_metadata,
};

#[path = "batch_executor_finish_check_aggregate.rs"]
mod batch_executor_finish_check_aggregate;

pub(crate) struct FreshBatchFinishContext {
    pub(crate) export_started: std::time::Instant,
    pub(crate) build_target_baseline_bytes: u64,
    pub(crate) process_residual_count: usize,
    pub(crate) test_binaries: Vec<RustTestBinaryIdentity>,
    pub(crate) repair_publication: Option<CheckAggregateRepairPublication>,
}

pub(crate) fn build_instance_results(
    started_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestStarted],
    ignored_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestStarted],
    terminal_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestTerminal],
    shim_metadata: &[BatchShimMetadata],
    exact: bool,
    req: &RustCoverageBatchRequest,
) -> Result<Vec<InstanceResult>, RustLlvmCovError> {
    reject_missing_terminal_events(started_tests, ignored_tests, terminal_tests, exact, req)?;
    let selector_index =
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::SelectorMatchIndex::new(
            &req.logical_selectors,
            exact,
        );
    let metadata_by_id: BTreeMap<_, _> = shim_metadata
        .iter()
        .map(|item| (item.full_name.clone(), item))
        .collect();
    let mut instances = Vec::new();
    for test in terminal_tests {
        if selector_index.matches(&test.full_name) {
            let shim = resolve_shim_metadata(&metadata_by_id, shim_metadata, &test.full_name)?;
            let exit_code = shim.exit_code.or(Some(if test.passed { 0 } else { 1 }));
            instances.push(InstanceResult {
                full_name: test.full_name.clone(),
                test_binary_id: test_binary_id_for_argv(&shim.argv)?,
                passed: test.passed,
                timed_out: test.timed_out,
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
    }
    instances.sort_by(|left, right| left.full_name.cmp(&right.full_name));
    Ok(instances)
}

pub(crate) fn build_instance_export_requests(
    instances: &[InstanceResult],
    shim_metadata: &[BatchShimMetadata],
    artifacts: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchCompilerArtifact],
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
    export_counters: crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::ExportCounters,
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
    let (mut completed, unmatched_selectors) =
        aggregate_with_zero_limit_bans(req, exact, &instances_with_coverage);
    let counters = RustCoverageBatchCounters {
        build_invocations: 1,
        test_instances: instances_with_coverage.len(),
        export_jobs: export_counters.export_jobs,
        max_active_test_instances: instances_with_coverage.len().min(req.jobs),
        max_active_exports: export_counters.max_active_exports,
        unmatched_selectors,
        max_objects_per_export: export_counters.max_objects_per_export,
        build_target_baseline_bytes: finish.build_target_baseline_bytes,
        export_phase_ms,
        process_residual_count: finish.process_residual_count,
        ..Default::default()
    };

    let kind = if req.population_publication_selectors.is_some() {
        "population coverage"
    } else {
        "selective coverage"
    };
    if let Some(err) = unmatched_selectors_batch_error(kind, unmatched_selectors, counters.clone())
    {
        return Ok(err);
    }
    match store_completed_outcomes(req, tools, identity, &mut completed) {
        Ok(()) => Ok(RustCoverageBatchResult {
            completed,
            batch_error: None,
            counters,
            test_binaries: finish.test_binaries,
        }),
        Err(store_err) => Ok(RustCoverageBatchResult {
            completed,
            batch_error: Some(store_err),
            counters,
            test_binaries: Vec::new(),
        }),
    }
}

pub(crate) fn finish_fresh_check_aggregate_after_export(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    export: FreshCheckAggregateExport,
    finish: FreshBatchFinishContext,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    let export_phase_ms = finish.export_started.elapsed().as_millis();
    let (completed, unmatched_selectors) =
        aggregate_with_zero_limit_bans(req, export.exact, &export.instances);
    let counters = RustCoverageBatchCounters {
        build_invocations: 1,
        test_instances: export.instances.len(),
        aggregate_binaries: export.exported.len(),
        aggregate_exports: export.counters.export_jobs,
        max_active_test_instances: export.instances.len().min(req.jobs),
        max_active_exports: export.counters.max_active_exports,
        unmatched_selectors,
        max_objects_per_export: export.counters.max_objects_per_export,
        build_target_baseline_bytes: finish.build_target_baseline_bytes,
        export_phase_ms,
        process_residual_count: finish.process_residual_count,
        ..Default::default()
    };
    if let Some(err) =
        unmatched_selectors_batch_error("check aggregate", unmatched_selectors, counters.clone())
    {
        return Ok(err);
    }
    if completed
        .iter()
        .any(|outcome| outcome.status != crate::rpytest_runner::TestStatus::Passed)
    {
        return Ok(RustCoverageBatchResult {
            completed,
            batch_error: None,
            counters,
            test_binaries: finish.test_binaries,
        });
    }
    batch_executor_finish_check_aggregate::store_and_publish_check_aggregate(
        req, tools, identity, export, finish, completed, counters,
    )
}

fn reject_missing_terminal_events(
    started_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestStarted],
    ignored_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestStarted],
    terminal_tests: &[crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::BatchTestTerminal],
    exact: bool,
    req: &RustCoverageBatchRequest,
) -> Result<(), RustLlvmCovError> {
    let terminal_names: BTreeSet<_> = terminal_tests
        .iter()
        .map(|test| test.full_name.as_str())
        .collect();
    let ignored_names: BTreeSet<_> =
        if crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::rust_test_args_include_ignored(&req.test_args) {
            BTreeSet::new()
        } else {
            ignored_tests
                .iter()
                .map(|test| test.full_name.as_str())
                .collect()
        };
    let selector_index =
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_events::SelectorMatchIndex::new(
            &req.logical_selectors,
            exact,
        );
    let missing = started_tests
        .iter()
        .filter(|test| {
            !terminal_names.contains(test.full_name.as_str())
                && !ignored_names.contains(test.full_name.as_str())
                && selector_index.matches(&test.full_name)
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

pub(crate) fn test_binary_id_for_argv(argv: &[String]) -> Result<String, RustLlvmCovError> {
    let executable = argv
        .first()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("missing test binary argv".into()))?;
    Ok(test_binary_id_for_path(Path::new(executable)))
}

pub(crate) fn test_binary_id_for_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn digest_test_binary(path: &Path) -> Result<String, RustLlvmCovError> {
    let bytes = std::fs::read(path).map_err(|err| {
        RustLlvmCovError::Io(std::io::Error::new(
            err.kind(),
            format!("digest_test_binary {}: {err}", path.display()),
        ))
    })?;
    let h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        &bytes,
    );
    Ok(format!("{h:016x}"))
}

pub(crate) fn test_binaries_from_shim_metadata(
    shim_metadata: &[BatchShimMetadata],
) -> Result<Vec<RustTestBinaryIdentity>, RustLlvmCovError> {
    let mut by_id = BTreeMap::new();
    for item in shim_metadata {
        if let Some(executable) = item.argv.first() {
            let path = PathBuf::from(executable);
            let id = test_binary_id_for_path(&path);

            if !by_id.contains_key(&id) {
                let digest = digest_test_binary(&path)?;
                by_id.insert(
                    id.clone(),
                    RustTestBinaryIdentity {
                        id,
                        executable: path.to_string_lossy().to_string(),
                        digest,
                    },
                );
            }
        }
    }
    Ok(by_id.into_values().collect())
}

#[cfg(test)]
#[path = "batch_executor_finish_test.rs"]
mod tests;
#[cfg(test)]
#[path = "batch_executor_finish_b_test.rs"]
mod tests_b;

#[cfg(test)]
#[path = "batch_executor_finish_digest_test.rs"]
mod digest_tests;

#[cfg(test)]
#[path = "batch_executor_finish_helpers_test.rs"]
mod test_helpers;
