use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::batch_fingerprint::RustCoverageToolIdentity;
use crate::batch_output_channel::{
    OutputChannelServer, apply_output_channel_env, create_output_channel_config,
};
use crate::batch_plan::RustCoverageBatchPlan;
use crate::batch_plan::RustCoverageBatchRequest;
use crate::batch_process_tree::{BatchProcessTreeGuard, record_child_process_group, wait_child};
use crate::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION, RustLlvmCovError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BuildIdentityPreparation {
    pub(crate) previous_baseline_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildIdentityFile {
    pub(crate) input: BuildIdentityInput,
    pub(crate) build_target_baseline_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuildIdentityInput {
    pub(crate) cache_schema: String,
    pub(crate) execution_policy: String,
    pub(crate) tool_versions: [String; 4],
    pub(crate) source_root: String,
    pub(crate) cargo_args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
}

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

pub fn run_batch_subprocess(
    cwd: &Path,
    plan: &RustCoverageBatchPlan,
) -> Result<BatchSubprocessRunOutcome, BatchSubprocessRunError> {
    ensure_batch_env_dirs(plan)?;
    let run_root = batch_run_root(plan)?;
    let (output_server, env) = start_output_channel_for_batch(run_root, plan)?;
    let process_tree = install_process_tree_guard()?;
    let started = Instant::now();
    let output = run_tracked_batch_command(cwd, plan, &env, &process_tree)?;
    let output_channel = output_server.stop_with_errors();
    if !output_channel.errors.is_empty() {
        return Err(spawn_component_error(
            "output-channel",
            output_channel.errors.join("; "),
        ));
    }
    Ok(BatchSubprocessRunOutcome {
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        duration: started.elapsed(),
        process_residual_count: process_tree.registry().residual_count(),
    })
}

fn batch_run_root(plan: &RustCoverageBatchPlan) -> Result<&Path, BatchSubprocessRunError> {
    plan.generated_config
        .parent()
        .ok_or_else(|| BatchSubprocessRunError::Spawn {
            program: "batch".to_string(),
            message: "generated nextest config path has no parent".to_string(),
        })
}

fn spawn_component_error(program: &str, message: String) -> BatchSubprocessRunError {
    BatchSubprocessRunError::Spawn {
        program: program.to_string(),
        message,
    }
}

fn start_output_channel_for_batch(
    run_root: &Path,
    plan: &RustCoverageBatchPlan,
) -> Result<(OutputChannelServer, BTreeMap<String, String>), BatchSubprocessRunError> {
    let channel_config = create_output_channel_config(run_root, plan.output_channel_relay_live)
        .map_err(|err| spawn_component_error("output-channel", err.to_string()))?;
    let mut env = plan.env.clone();
    apply_output_channel_env(&mut env, &channel_config);
    let output_server = OutputChannelServer::start(channel_config)
        .map_err(|err| spawn_component_error("output-channel", err.to_string()))?;
    Ok((output_server, env))
}

fn install_process_tree_guard() -> Result<BatchProcessTreeGuard, BatchSubprocessRunError> {
    BatchProcessTreeGuard::install()
        .map_err(|err| spawn_component_error("process-tree", err.to_string()))
}

fn run_tracked_batch_command(
    cwd: &Path,
    plan: &RustCoverageBatchPlan,
    env: &BTreeMap<String, String>,
    process_tree: &BatchProcessTreeGuard,
) -> Result<std::process::Output, BatchSubprocessRunError> {
    let argv = &plan.argv;
    let program = argv.first().cloned().unwrap_or_else(|| "cargo".to_string());
    let mut command = Command::new(&program);
    command
        .args(&argv[1..])
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process_tree
        .spawn_batch_command(&mut command)
        .map_err(|err| spawn_component_error(&program, err.to_string()))?;
    record_child_process_group(process_tree.registry().as_ref(), &child);
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| spawn_component_error(&program, "missing stdout pipe".to_string()))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| spawn_component_error(&program, "missing stderr pipe".to_string()))?;
    let stdout_handle = std::thread::spawn(move || read_pipe_to_end(stdout_pipe));
    let stderr_handle = std::thread::spawn(move || read_pipe_to_end(stderr_pipe));
    let status =
        wait_child(&mut child).map_err(|err| spawn_component_error(&program, err.to_string()))?;
    let stdout = join_pipe_reader(stdout_handle, &program, "stdout")?;
    let stderr = join_pipe_reader(stderr_handle, &program, "stderr")?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn join_pipe_reader(
    handle: std::thread::JoinHandle<io::Result<Vec<u8>>>,
    program: &str,
    label: &str,
) -> Result<Vec<u8>, BatchSubprocessRunError> {
    handle
        .join()
        .map_err(|_| spawn_component_error(program, format!("{label} reader panicked")))?
        .map_err(|err| spawn_component_error(program, err.to_string()))
}

fn read_pipe_to_end<R: io::Read>(mut pipe: R) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn ensure_batch_env_dirs(plan: &RustCoverageBatchPlan) -> Result<(), BatchSubprocessRunError> {
    for key in [
        "CARGO_TARGET_DIR",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_BUILD_DIR",
    ] {
        if let Some(path) = plan.env.get(key) {
            fs::create_dir_all(path).map_err(|err| BatchSubprocessRunError::Spawn {
                program: plan
                    .argv
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "cargo".to_string()),
                message: format!("failed to create {key}={path}: {err}"),
            })?;
        }
    }
    Ok(())
}

const BUILD_TARGET_GROWTH_NUMERATOR: u64 = 3;
const BUILD_TARGET_GROWTH_DENOMINATOR: u64 = 2;

pub(crate) fn prepare_build_target_for_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
) -> io::Result<BuildIdentityPreparation> {
    let expected = build_identity_input(req, tools);
    if let Some(previous) = load_build_identity(&req.cache_root)?
        && previous.input == expected
    {
        let baseline = previous.build_target_baseline_bytes;
        if baseline > 0 {
            let current_bytes = path_size_bytes(&plan.build_target)?;
            let growth_limit = baseline.saturating_mul(BUILD_TARGET_GROWTH_NUMERATOR)
                / BUILD_TARGET_GROWTH_DENOMINATOR;
            if current_bytes > growth_limit {
                remove_build_target(&plan.build_target)?;
                return Ok(BuildIdentityPreparation {
                    previous_baseline_bytes: 0,
                });
            }
        }
        return Ok(BuildIdentityPreparation {
            previous_baseline_bytes: baseline,
        });
    }
    remove_build_target(&plan.build_target)?;
    Ok(BuildIdentityPreparation {
        previous_baseline_bytes: 0,
    })
}

pub(crate) fn publish_successful_build_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    plan: &RustCoverageBatchPlan,
    previous_baseline_bytes: u64,
) -> io::Result<u64> {
    let current_target_bytes = path_size_bytes(&plan.build_target)?;
    let baseline_bytes = if previous_baseline_bytes == 0 {
        current_target_bytes
    } else {
        previous_baseline_bytes
    };
    let marker = BuildIdentityFile {
        input: build_identity_input(req, tools),
        build_target_baseline_bytes: baseline_bytes,
    };
    write_build_identity_atomic(&req.cache_root, &marker)?;
    Ok(baseline_bytes)
}

pub(crate) fn build_identity_input(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> BuildIdentityInput {
    BuildIdentityInput {
        cache_schema: CACHE_SCHEMA_VERSION.to_string(),
        execution_policy: BATCH_EXECUTION_POLICY_VERSION.to_string(),
        tool_versions: [
            tools.cargo_version.clone(),
            tools.llvm_cov_version.clone(),
            tools.rustc_version.clone(),
            tools.cargo_nextest_version.clone(),
        ],
        source_root: req.source_root.to_string_lossy().to_string(),
        cargo_args: req.cargo_args.clone(),
        env: req.env.clone(),
    }
}

fn load_build_identity(cache_root: &Path) -> io::Result<Option<BuildIdentityFile>> {
    let path = build_identity_path(cache_root);
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(io::Error::other),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_build_identity_atomic(cache_root: &Path, marker: &BuildIdentityFile) -> io::Result<()> {
    let path = build_identity_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("build identity path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(marker).map_err(io::Error::other)?,
    )?;
    fs::rename(tmp, path)
}

pub(crate) fn build_identity_path(cache_root: &Path) -> PathBuf {
    cache_root.join("build").join("identity.json")
}

fn remove_build_target(build_target: &Path) -> io::Result<()> {
    match fs::remove_dir_all(build_target) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn path_size_bytes(path: &Path) -> io::Result<u64> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err),
    };
    if meta.is_file() {
        return Ok(meta.len());
    }
    if meta.is_dir() {
        return fs::read_dir(path)?.try_fold(0, |total, entry| {
            Ok(total + path_size_bytes(&entry?.path())?)
        });
    }
    Ok(0)
}

#[cfg(test)]
#[path = "batch_run_test.rs"]
mod tests;
