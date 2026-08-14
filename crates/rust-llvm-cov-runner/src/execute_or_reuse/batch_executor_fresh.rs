use crate::{
    InstanceResult, RustLlvmCovError, RustTestBinaryIdentity,
    batch_check_aggregate_export::{
        build_check_aggregate_export_requests, export_check_aggregates_bounded,
    },
    batch_events::{BatchEventStream, parse_batch_event_stream},
    batch_executor_finish::{
        FreshBatchFinishContext, FreshCheckAggregateExport as CheckExport,
        build_instance_export_requests, build_instance_results, finish_fresh_batch_after_export,
        finish_fresh_check_aggregate_after_export, test_binaries_from_shim_metadata,
    },
    batch_export::{SubprocessInstanceExporter, export_instances_bounded},
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity},
    batch_plan::{CoverageOutputMode, RustCoverageBatchPlan, RustCoverageBatchRequest},
    batch_result::RustCoverageBatchResult,
    batch_run::{
        self, BatchSubprocessRunner, BuildIdentityPreparation, CurrentRunCleanup,
        FreshBatchRunScope,
    },
    batch_shim::load_target_runner_shim_metadata,
};

pub(crate) fn execute_fresh_batch_with_exporter(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    exporter: SubprocessInstanceExporter,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    execute_fresh_batch_with_cleanup(
        req,
        tools,
        identity,
        plan,
        runner,
        CurrentRunCleanup::default(),
        |req, source_root, object_catalog, export_requests| {
            let exporter = exporter.with_catalog_map(object_catalog)?;
            export_instances_bounded(
                req.jobs,
                exporter,
                source_root,
                object_catalog,
                export_requests,
            )
        },
    )
}

#[cfg(test)]
use crate::execute_or_reuse::batch_export::export_instances_bounded_with;

#[cfg(test)]
pub(crate) fn execute_fresh_batch_with_export_fn(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    export_fn: crate::execute_or_reuse::batch_export::BatchInstanceExportFn,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    execute_fresh_batch_with_cleanup(
        req,
        tools,
        identity,
        plan,
        runner,
        CurrentRunCleanup::default(),
        |req, source_root, object_catalog, export_requests| {
            export_instances_bounded_with(
                req.jobs,
                source_root,
                object_catalog,
                export_requests,
                export_fn,
            )
        },
    )
}

#[cfg(test)]
pub(crate) fn execute_fresh_batch_with_export_fn_and_cleanup(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    export_fn: crate::execute_or_reuse::batch_export::BatchInstanceExportFn,
    cleanup: CurrentRunCleanup,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    execute_fresh_batch_with_cleanup(
        req,
        tools,
        identity,
        plan,
        runner,
        cleanup,
        |req, source_root, object_catalog, export_requests| {
            export_instances_bounded_with(
                req.jobs,
                source_root,
                object_catalog,
                export_requests,
                export_fn,
            )
        },
    )
}

fn execute_fresh_batch_with_cleanup<E>(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    cleanup: CurrentRunCleanup,
    export_step: E,
) -> Result<RustCoverageBatchResult, RustLlvmCovError>
where
    E: FnOnce(
        &RustCoverageBatchRequest,
        &std::path::Path,
        &[std::path::PathBuf],
        Vec<crate::execute_or_reuse::batch_export::InstanceExportRequest>,
    ) -> Result<
        (
            Vec<(String, crate::RustLineCoverage)>,
            crate::execute_or_reuse::batch_export::ExportCounters,
        ),
        RustLlvmCovError,
    >,
{
    crate::plan::batch_platform::ensure_batch_platform_supported()?;
    let scope = FreshBatchRunScope::begin_with_layout(&req.cache_root, plan, cleanup)
        .map_err(RustLlvmCovError::from)?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let outcome = (|| -> Result<RustCoverageBatchResult, RustLlvmCovError> {
        let prepared = prepare_fresh_batch_run(req, tools, plan, runner, build_identity)?;
        let export_started = std::time::Instant::now();
        match &req.coverage_output_mode {
            CoverageOutputMode::SelectorEntries => {
                let export_requests = build_instance_export_requests(
                    &prepared.instances,
                    &prepared.shim_metadata,
                    &prepared.parsed.compiler_artifacts,
                )?;
                let (exported, export_counters) =
                    crate::execute_or_reuse::progress::log_named_step("llvm-cov", || {
                        let object_catalog =
                            crate::execute_or_reuse::batch_export_catalog::build_object_catalog(
                                &prepared.parsed.compiler_artifacts,
                                &plan.build_target,
                                &export_requests,
                                &req.env,
                            );
                        export_step(req, &req.source_root, &object_catalog, export_requests)
                    })?;
                finish_fresh_batch_after_export(
                    req,
                    tools,
                    identity,
                    prepared.exact,
                    prepared.instances,
                    exported,
                    export_counters,
                    FreshBatchFinishContext {
                        export_started,
                        build_target_baseline_bytes: prepared.build_target_baseline_bytes,
                        process_residual_count: prepared.process_residual_count,
                        test_binaries: prepared.test_binaries,
                        repair_publication: None,
                    },
                )
            }
            CoverageOutputMode::CheckAggregate {
                publication_binary_ids,
                repair_publication,
            } => {
                let aggregate_requests = build_check_aggregate_export_requests(
                    &prepared.instances,
                    &prepared.shim_metadata,
                    &prepared.parsed.compiler_artifacts,
                    publication_binary_ids.as_ref(),
                )?;
                let (exported, export_counters) =
                    crate::execute_or_reuse::progress::log_named_step("llvm-cov", || {
                        let mut object_catalog =
                            crate::execute_or_reuse::batch_export_catalog::build_object_catalog(
                                &prepared.parsed.compiler_artifacts,
                                &plan.build_target,
                                &[],
                                &req.env,
                            );
                        for request in &aggregate_requests {
                            object_catalog.extend(request.objects.iter().cloned());
                        }
                        object_catalog.sort();
                        object_catalog.dedup();
                        export_check_aggregates_bounded(
                            req.jobs,
                            &req.source_root,
                            &object_catalog,
                            aggregate_requests,
                        )
                    })?;
                finish_fresh_check_aggregate_after_export(
                    req,
                    tools,
                    identity,
                    CheckExport::new(
                        prepared.exact,
                        prepared.instances,
                        exported,
                        export_counters,
                    ),
                    FreshBatchFinishContext {
                        export_started,
                        build_target_baseline_bytes: prepared.build_target_baseline_bytes,
                        process_residual_count: prepared.process_residual_count,
                        test_binaries: prepared.test_binaries,
                        repair_publication: repair_publication.clone(),
                    },
                )
            }
        }
    })();
    match outcome {
        Ok(result) => scope.finish_batch_result(result),
        Err(err) => scope.finish(Err(err)),
    }
}

struct PreparedFreshBatchRun {
    parsed: BatchEventStream,
    exact: bool,
    shim_metadata: Vec<crate::execute_or_reuse::batch_shim::BatchShimMetadata>,
    test_binaries: Vec<RustTestBinaryIdentity>,
    instances: Vec<InstanceResult>,
    build_target_baseline_bytes: u64,
    process_residual_count: usize,
}

fn prepare_fresh_batch_run(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    build_identity: BuildIdentityPreparation,
) -> Result<PreparedFreshBatchRun, RustLlvmCovError> {
    crate::plan::batch_runner_resolve::write_runner_map(
        &plan.runner_map_path,
        &req.delegated_runners,
    )?;
    crate::plan::batch_plan_publish::publish_generated_nextest_config(plan, req)?;
    let run = runner.run(&req.cwd, plan).map_err(RustLlvmCovError::from)?;
    let parsed = parse_batch_event_stream(&run.stdout)?;
    reject_failed_build_without_tests(&run, &parsed)?;
    reject_nonzero_without_terminal_events(&run, &parsed)?;
    if batch_run::batch_scope_interrupted() {
        return Err(RustLlvmCovError::Interrupted);
    }
    let build_target_baseline_bytes = batch_run::publish_successful_build_identity(
        req,
        tools,
        plan,
        build_identity.previous_baseline_bytes,
    )?;
    let exact = req.test_args.iter().any(|arg| arg == "--exact");
    let shim_metadata = match &req.coverage_output_mode {
        CoverageOutputMode::CheckAggregate { .. } => {
            let run_root = plan
                .generated_config
                .parent()
                .unwrap_or(req.cache_root.as_path());
            let profile = crate::execute_or_reuse::batch_shim_synthesize::check_aggregate_pool_profile_path_for_run(
                &plan.build_target,
                run_root,
            );
            crate::execute_or_reuse::batch_shim_synthesize::synthesize_check_aggregate_shim_metadata(
                &parsed,
                &profile,
                &req.cwd,
            )?
        }
        CoverageOutputMode::SelectorEntries => {
            load_target_runner_shim_metadata(&plan.target_runner_output_dir)?
        }
    };
    let test_binaries = test_binaries_from_shim_metadata(&shim_metadata)?;
    let instances = build_instance_results(
        &parsed.started_tests,
        &parsed.ignored_tests,
        &parsed.terminal_tests,
        &shim_metadata,
        exact,
        req,
    )?;
    Ok(PreparedFreshBatchRun {
        parsed,
        exact,
        shim_metadata,
        test_binaries,
        instances,
        build_target_baseline_bytes,
        process_residual_count: run.process_residual_count,
    })
}

fn reject_failed_build_without_tests(
    run: &crate::execute_or_reuse::batch_run::BatchSubprocessRunOutcome,
    parsed: &BatchEventStream,
) -> Result<(), RustLlvmCovError> {
    if parsed.build_succeeded.unwrap_or(false) {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&run.stderr);
    Err(RustLlvmCovError::InvalidRequest(format!(
        "nextest batch build failed before test execution: {detail}"
    )))
}

#[cfg(test)]
pub(crate) fn apply_non_primary_cleanup_error(
    result: RustCoverageBatchResult,
    stale_cleanup_error: Option<std::io::Error>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    batch_run::finalize_batch_result(result, stale_cleanup_error, None)
}

pub(crate) fn reject_nonzero_without_terminal_events(
    run: &crate::execute_or_reuse::batch_run::BatchSubprocessRunOutcome,
    parsed: &crate::execute_or_reuse::batch_events::BatchEventStream,
) -> Result<(), RustLlvmCovError> {
    if run.exit_code == Some(0) || !parsed.terminal_tests.is_empty() {
        return Ok(());
    }
    Err(RustLlvmCovError::InvalidRequest(format!(
        "nextest batch exited {:?} without terminal test events: {}",
        run.exit_code,
        String::from_utf8_lossy(&run.stderr)
    )))
}

#[cfg(test)]
#[path = "batch_executor_fresh_helpers_test.rs"]
pub(crate) mod fresh_test_helpers;

#[cfg(test)]
#[path = "batch_executor_fresh_cleanup_test.rs"]
mod fresh_cleanup_tests;
#[cfg(test)]
#[path = "batch_executor_fresh_test.rs"]
mod fresh_tests;
