use std::fs;
use std::io;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::RustLlvmCovError;
use crate::plan::batch_plan::RustCoverageBatchPlan;

#[path = "batch_run_cleanup.rs"]
mod batch_run_cleanup;
#[path = "batch_run_identity.rs"]
mod batch_run_identity;
#[path = "batch_run_subprocess.rs"]
mod batch_run_subprocess;

pub(crate) use crate::execute_or_reuse::batch_process_tree::batch_scope_interrupted;
#[cfg(test)]
pub(crate) use batch_run_cleanup::finalize_batch_result;
pub(crate) use batch_run_cleanup::{CurrentRunCleanup, FreshBatchRunScope};

#[allow(unused_imports)]
pub(crate) use batch_run_identity::{
    BuildIdentityFile, BuildIdentityInput, BuildIdentityPreparation, build_identity_input,
    build_identity_path, path_size_bytes, prepare_build_target_for_identity,
    publish_successful_build_identity,
};
pub(crate) use batch_run_subprocess::run_batch_subprocess;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchSubprocessRunOutcome {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration: Duration,
    pub process_residual_count: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BatchSubprocessRunError {
    Spawn { program: String, message: String },
}

impl From<BatchSubprocessRunError> for RustLlvmCovError {
    fn from(value: BatchSubprocessRunError) -> Self {
        match value {
            BatchSubprocessRunError::Spawn { program, message } => {
                Self::InvalidRequest(format!("failed to spawn `{program}`: {message}"))
            }
        }
    }
}

type BatchRunnerFn = dyn Fn(
    &Path,
    &RustCoverageBatchPlan,
) -> Result<BatchSubprocessRunOutcome, BatchSubprocessRunError>;

#[derive(Clone)]
pub struct BatchSubprocessRunner {
    run: Arc<BatchRunnerFn>,
}

impl BatchSubprocessRunner {
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn(
                &Path,
                &RustCoverageBatchPlan,
            ) -> Result<BatchSubprocessRunOutcome, BatchSubprocessRunError>
            + 'static,
    {
        Self { run: Arc::new(f) }
    }

    pub fn run(
        &self,
        cwd: &Path,
        plan: &RustCoverageBatchPlan,
    ) -> Result<BatchSubprocessRunOutcome, BatchSubprocessRunError> {
        (self.run)(cwd, plan)
    }
}

pub fn default_batch_subprocess_runner() -> BatchSubprocessRunner {
    BatchSubprocessRunner::from_fn(run_batch_subprocess)
}

#[cfg(test)]
pub fn prepare_batch_run_layout(plan: &RustCoverageBatchPlan) -> io::Result<PathBuf> {
    let run_root = plan
        .generated_config
        .parent()
        .ok_or_else(|| io::Error::other("generated nextest config path has no parent"))?
        .to_path_buf();
    fs::create_dir_all(&run_root)?;
    fs::create_dir_all(&plan.target_runner_output_dir)?;
    Ok(run_root)
}

pub fn remove_stale_run_directories(cache_root: &Path, keep_run_root: &Path) -> io::Result<()> {
    let runs_root = cache_root.join("runs");
    if !runs_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&runs_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || path == keep_run_root {
            continue;
        }
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "batch_run_test.rs"]
mod tests;
