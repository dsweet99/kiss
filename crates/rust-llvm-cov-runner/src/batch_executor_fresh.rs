use crate::{
    RustLlvmCovError,
    batch_events::parse_batch_event_stream,
    batch_executor_finish::{
        FreshBatchFinishContext, build_instance_export_requests, build_instance_results,
        finish_fresh_batch_after_export,
    },
    batch_export::{SubprocessInstanceExporter, export_instances_bounded},
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity},
    batch_plan::{RustCoverageBatchPlan, RustCoverageBatchRequest},
    batch_result::RustCoverageBatchResult,
    batch_run::{self, BatchSubprocessRunner},
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
    crate::batch_platform::ensure_batch_platform_supported()?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let run_root = batch_run::prepare_batch_run_layout(plan)?;
    let stale_cleanup_error =
        batch_run::remove_stale_run_directories(&req.cache_root, &run_root).err();
    crate::batch_runner_resolve::write_runner_map(&plan.runner_map_path, &req.delegated_runners)?;
    crate::batch_plan_publish::publish_generated_nextest_config(plan)?;
    let run = runner.run(&req.cwd, plan).map_err(RustLlvmCovError::from)?;
    let parsed = parse_batch_event_stream(&run.stdout)?;
    let build_succeeded = parsed.build_succeeded.unwrap_or(false);
    if !build_succeeded {
        let detail = String::from_utf8_lossy(&run.stderr);
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "nextest batch build failed before test execution: {detail}"
        )));
    }
    reject_nonzero_without_terminal_events(&run, &parsed)?;
    let build_target_baseline_bytes = batch_run::publish_successful_build_identity(
        req,
        tools,
        plan,
        build_identity.previous_baseline_bytes,
    )?;
    let exact = req.test_args.iter().any(|arg| arg == "--exact");
    let shim_metadata = load_target_runner_shim_metadata(&plan.target_runner_output_dir)?;
    let instances = build_instance_results(
        &parsed.started_tests,
        &parsed.ignored_tests,
        &parsed.terminal_tests,
        &shim_metadata,
        exact,
        req,
    )?;
    let export_requests =
        build_instance_export_requests(&instances, &shim_metadata, &parsed.compiler_artifacts)?;
    let object_catalog = crate::batch_export_catalog::build_object_catalog(
        &parsed.compiler_artifacts,
        &plan.build_target,
        &export_requests,
        &req.env,
    );
    let export_started = std::time::Instant::now();
    let exporter = exporter.with_catalog_map(&object_catalog)?;
    let (exported, export_counters) = export_instances_bounded(
        req.jobs,
        exporter,
        &req.source_root,
        &object_catalog,
        export_requests,
    )?;
    finish_fresh_batch_after_export(
        req,
        tools,
        identity,
        exact,
        instances,
        exported,
        export_counters,
        FreshBatchFinishContext {
            export_started,
            build_target_baseline_bytes,
            process_residual_count: run.process_residual_count,
        },
    )
    .and_then(|result| apply_non_primary_cleanup_error(result, stale_cleanup_error))
}

#[cfg(test)]
use crate::batch_export::export_instances_bounded_with;

#[cfg(test)]
pub(crate) fn execute_fresh_batch_with_export_fn(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    plan: &RustCoverageBatchPlan,
    runner: &BatchSubprocessRunner,
    export_fn: crate::batch_export::BatchInstanceExportFn,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    crate::batch_platform::ensure_batch_platform_supported()?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let run_root = batch_run::prepare_batch_run_layout(plan)?;
    let stale_cleanup_error =
        batch_run::remove_stale_run_directories(&req.cache_root, &run_root).err();
    crate::batch_runner_resolve::write_runner_map(&plan.runner_map_path, &req.delegated_runners)?;
    crate::batch_plan_publish::publish_generated_nextest_config(plan)?;
    let run = runner.run(&req.cwd, plan).map_err(RustLlvmCovError::from)?;
    let parsed = parse_batch_event_stream(&run.stdout)?;
    let build_succeeded = parsed.build_succeeded.unwrap_or(false);
    if !build_succeeded {
        let detail = String::from_utf8_lossy(&run.stderr);
        return Err(RustLlvmCovError::InvalidRequest(format!(
            "nextest batch build failed before test execution: {detail}"
        )));
    }
    reject_nonzero_without_terminal_events(&run, &parsed)?;
    let build_target_baseline_bytes = batch_run::publish_successful_build_identity(
        req,
        tools,
        plan,
        build_identity.previous_baseline_bytes,
    )?;
    let exact = req.test_args.iter().any(|arg| arg == "--exact");
    let shim_metadata = load_target_runner_shim_metadata(&plan.target_runner_output_dir)?;
    let instances = build_instance_results(
        &parsed.started_tests,
        &parsed.ignored_tests,
        &parsed.terminal_tests,
        &shim_metadata,
        exact,
        req,
    )?;
    let export_requests =
        build_instance_export_requests(&instances, &shim_metadata, &parsed.compiler_artifacts)?;
    let object_catalog = crate::batch_export_catalog::build_object_catalog(
        &parsed.compiler_artifacts,
        &plan.build_target,
        &export_requests,
        &req.env,
    );
    let export_started = std::time::Instant::now();
    let (exported, export_counters) = export_instances_bounded_with(
        req.jobs,
        &req.source_root,
        &object_catalog,
        export_requests,
        export_fn,
    )?;
    finish_fresh_batch_after_export(
        req,
        tools,
        identity,
        exact,
        instances,
        exported,
        export_counters,
        FreshBatchFinishContext {
            export_started,
            build_target_baseline_bytes,
            process_residual_count: run.process_residual_count,
        },
    )
    .and_then(|result| apply_non_primary_cleanup_error(result, stale_cleanup_error))
}

pub(crate) fn apply_non_primary_cleanup_error(
    result: RustCoverageBatchResult,
    stale_cleanup_error: Option<std::io::Error>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    if result.batch_error.is_some() {
        return Ok(result);
    }
    if let Some(err) = stale_cleanup_error {
        return Err(RustLlvmCovError::Io(err));
    }
    Ok(result)
}

fn reject_nonzero_without_terminal_events(
    run: &crate::batch_run::BatchSubprocessRunOutcome,
    parsed: &crate::batch_events::BatchEventStream,
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
