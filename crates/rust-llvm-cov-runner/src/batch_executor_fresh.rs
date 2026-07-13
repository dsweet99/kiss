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
    batch_run::{self, BatchSubprocessRunner, CurrentRunCleanup, FreshBatchRunScope},
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
    export_fn: crate::batch_export::BatchInstanceExportFn,
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
        Vec<crate::batch_export::InstanceExportRequest>,
    ) -> Result<
        (
            Vec<(String, crate::RustLineCoverage)>,
            crate::batch_export::ExportCounters,
        ),
        RustLlvmCovError,
    >,
{
    crate::batch_platform::ensure_batch_platform_supported()?;
    let scope = FreshBatchRunScope::begin_with_layout(&req.cache_root, plan, cleanup)
        .map_err(RustLlvmCovError::from)?;
    let build_identity = batch_run::prepare_build_target_for_identity(req, tools, plan)?;
    let outcome = (|| -> Result<RustCoverageBatchResult, RustLlvmCovError> {
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
        if batch_run::batch_scope_interrupted() {
            return Err(RustLlvmCovError::InvalidRequest(
                "batch interrupted".into(),
            ));
        }
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
        let export_requests = build_instance_export_requests(
            &instances,
            &shim_metadata,
            &parsed.compiler_artifacts,
        )?;
        let object_catalog = crate::batch_export_catalog::build_object_catalog(
            &parsed.compiler_artifacts,
            &plan.build_target,
            &export_requests,
            &req.env,
        );
        let export_started = std::time::Instant::now();
        let (exported, export_counters) = export_step(
            req,
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
    })();
    match outcome {
        Ok(result) => scope.finish_batch_result(result),
        Err(err) => scope.finish(Err(err)),
    }
}

#[cfg(test)]
pub(crate) fn apply_non_primary_cleanup_error(
    result: RustCoverageBatchResult,
    stale_cleanup_error: Option<std::io::Error>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    batch_run::finalize_batch_result(result, stale_cleanup_error, None)
}

pub(crate) fn reject_nonzero_without_terminal_events(
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

#[cfg(test)]
pub(crate) mod fresh_test_helpers {
    use crate::RustLineCoverage;
    use crate::RustLlvmCovError;
    use crate::batch_export::{FakeInstanceExporter, write_fake_profile};
    use crate::batch_fingerprint::batch_identity;
    use crate::batch_lock::lock_batch;
    use crate::batch_plan::{RustCoverageBatchRequest, build_rust_coverage_batch_plan};
    use crate::batch_result::RustCoverageBatchResult;
    use crate::batch_run::BatchSubprocessRunner;
    use crate::batch_shim::BatchShimMetadata;
    use crate::test_support::witness_batch_tools;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    pub(crate) fn tools() -> crate::RustCoverageToolIdentity {
        witness_batch_tools()
    }

    pub(crate) fn fake_runner() -> BatchSubprocessRunner {
        BatchSubprocessRunner::from_fn(|_, plan| {
            fs::create_dir_all(&plan.build_target).unwrap();
            fs::write(plan.build_target.join("artifact"), b"target").unwrap();
            write_shim_metadata(&plan.target_runner_output_dir, "pkg::bin$alpha");
            Ok(crate::batch_run::BatchSubprocessRunOutcome {
                exit_code: Some(0),
                stdout: br#"{"reason":"compiler-artifact","executable":"/tmp/bin","filenames":["/tmp/a.o"],"fresh":false}
{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$alpha","exec_time":0.001}
"#
                .to_vec(),
                stderr: Vec::new(),
                duration: Duration::from_millis(1),
                process_residual_count: 0,
            })
        })
    }

    pub(crate) fn execute_rust_coverage_batch_fresh_with_fake(
        req: &RustCoverageBatchRequest,
        runner: BatchSubprocessRunner,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
        let tools = tools();
        let identity = batch_identity(req, &tools)?;
        let plan = build_rust_coverage_batch_plan(req)
            .map_err(|message| RustLlvmCovError::InvalidRequest(format!("batch plan: {message}")))?;
        let _batch_guard = lock_batch(&req.cache_root)?;
        let mut coverage = BTreeMap::new();
        coverage.insert(
            "pkg::bin$alpha".to_string(),
            RustLineCoverage {
                files: BTreeMap::from([(
                    "src/lib.rs".to_string(),
                    std::collections::BTreeSet::from([1]),
                )]),
            },
        );
        let fake = Arc::new(FakeInstanceExporter::new(coverage));
        super::execute_fresh_batch_with_export_fn(
            req,
            &tools,
            &identity,
            &plan,
            &runner,
            Arc::new(
                move |batch_executor_request, source_root, _catalog, seed_objects| {
                    fake.export_instance(batch_executor_request, source_root, &[], seed_objects)
                },
            ),
        )
    }

    pub(crate) fn write_shim_metadata(output_dir: &Path, id: &str) {
        fs::create_dir_all(output_dir).unwrap();
        let profile_path = output_dir.join(format!("{id}.profraw"));
        write_fake_profile(&profile_path, b"profile").unwrap();
        let metadata = BatchShimMetadata {
            schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
            id: id.to_string(),
            full_name: id.to_string(),
            profile_path,
            cwd: output_dir.to_path_buf(),
            argv: vec!["/tmp/bin".to_string()],
            exit_code: Some(0),
            spawn_error: None,
            shim_identity: None,
            delegated_identity: None,
            stdout: None,
            stderr: None,
            output_frame_count: None,
        };
        fs::write(
            output_dir.join(format!("{id}.json")),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
    }

    pub(crate) fn run_root_for(req: &RustCoverageBatchRequest) -> PathBuf {
        req.generated_config.parent().unwrap().to_path_buf()
    }
}

#[cfg(test)]
#[path = "batch_executor_fresh_test.rs"]
mod fresh_tests;

#[cfg(test)]
#[path = "batch_executor_fresh_cleanup_test.rs"]
mod fresh_cleanup_tests;
