use crate::RustLineCoverage;
use crate::RustLlvmCovError;
use crate::execute_or_reuse::batch_export::{FakeInstanceExporter, write_fake_profile};
use crate::plan::batch_fingerprint::batch_identity;
use crate::execute_or_reuse::batch_lock::lock_batch;
use crate::plan::batch_plan::{RustCoverageBatchRequest, build_rust_coverage_batch_plan};
use crate::execute_or_reuse::batch_result::RustCoverageBatchResult;
use crate::execute_or_reuse::batch_run::BatchSubprocessRunner;
use crate::execute_or_reuse::batch_shim::BatchShimMetadata;
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
        let bin = plan.build_target.join("bin");
        fs::write(&bin, b"binary").unwrap();
        write_shim_metadata(&plan.target_runner_output_dir, "pkg::bin$alpha", &bin);
        write_shim_metadata(&plan.target_runner_output_dir, "pkg::bin$beta", &bin);
        Ok(crate::execute_or_reuse::batch_run::BatchSubprocessRunOutcome {
            exit_code: Some(0),
            stdout: format!(
                "{{\"reason\":\"compiler-artifact\",\"executable\":\"{}\",\"filenames\":[\"/tmp/a.o\"],\"fresh\":false}}\n{{\"reason\":\"build-finished\",\"success\":true}}\n{{\"type\":\"test\",\"event\":\"ok\",\"name\":\"pkg::bin$alpha\",\"exec_time\":0.001}}\n{{\"type\":\"test\",\"event\":\"ok\",\"name\":\"pkg::bin$beta\",\"exec_time\":0.001}}\n",
                bin.display()
            )
            .into_bytes(),
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
    let coverage_files = RustLineCoverage {
        files: BTreeMap::from([(
            "src/lib.rs".to_string(),
            std::collections::BTreeSet::from([1]),
        )]),
    };
    let mut coverage = BTreeMap::new();
    coverage.insert("pkg::bin$alpha".to_string(), coverage_files.clone());
    coverage.insert("pkg::bin$beta".to_string(), coverage_files);
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

pub(crate) fn write_shim_metadata(output_dir: &Path, id: &str, bin: &Path) {
    fs::create_dir_all(output_dir).unwrap();
    let profile_path = output_dir.join(format!("{id}.profraw"));
    write_fake_profile(&profile_path, b"profile").unwrap();
    let metadata = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v1".to_string(),
        id: id.to_string(),
        full_name: id.to_string(),
        profile_path,
        cwd: output_dir.to_path_buf(),
        argv: vec![bin.to_string_lossy().to_string()],
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
