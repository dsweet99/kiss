use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::batch_output_channel::{
    OutputChannelServer, apply_output_channel_env, create_output_channel_config,
};
use crate::batch_plan::RustCoverageBatchPlan;
use crate::batch_process_tree::{BatchProcessTreeGuard, record_child_process_group};
use crate::batch_shim::load_live_shim_process_identities;

use super::{BatchSubprocessRunError, BatchSubprocessRunOutcome};

struct OutputChannelShutdown {
    server: Option<OutputChannelServer>,
}

impl OutputChannelShutdown {
    fn new(server: OutputChannelServer) -> Self {
        Self {
            server: Some(server),
        }
    }

    fn stop_with_errors(mut self) -> crate::batch_output_channel::OutputChannelStop {
        self.server
            .take()
            .expect("output channel server present")
            .stop_with_errors()
    }
}

impl Drop for OutputChannelShutdown {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop_with_errors();
        }
    }
}

pub(crate) fn run_batch_subprocess(
    cwd: &Path,
    plan: &RustCoverageBatchPlan,
) -> Result<BatchSubprocessRunOutcome, BatchSubprocessRunError> {
    ensure_batch_env_dirs(plan)?;
    let run_root = batch_run_root(plan)?;
    let (output_server, env) = start_output_channel_for_batch(run_root, plan)?;
    let output_server = OutputChannelShutdown::new(output_server);
    let process_tree = install_process_tree_guard()?;
    let started = std::time::Instant::now();
    let output = run_tracked_batch_command(cwd, plan, &env, &process_tree)?;
    let output_channel = output_server.stop_with_errors();
    if !output_channel.errors.is_empty() {
        return Err(spawn_component_error(
            "output-channel",
            output_channel.errors.join("; "),
        ));
    }
    let process_residual_count = if process_tree.interrupted() {
        ingest_live_shim_identities(
            process_tree.registry().as_ref(),
            &plan.target_runner_output_dir,
            &mut HashSet::new(),
        );
        process_tree.terminate_descendants(Duration::from_millis(250))
    } else {
        process_tree.registry().residual_count()
    };
    Ok(BatchSubprocessRunOutcome {
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        duration: started.elapsed(),
        process_residual_count,
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
    let output_dir = plan.target_runner_output_dir.clone();
    let mut seen_shim_metadata = HashSet::new();
    let wait_result = wait_child_with_interruption(
        &mut child,
        process_tree,
        &output_dir,
        &mut seen_shim_metadata,
    );
    let stdout = join_pipe_reader(stdout_handle, &program, "stdout")?;
    let stderr = join_pipe_reader(stderr_handle, &program, "stderr")?;
    let status = wait_result.map_err(|err| spawn_component_error(&program, err.to_string()))?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn wait_child_with_interruption(
    child: &mut std::process::Child,
    process_tree: &BatchProcessTreeGuard,
    output_dir: &Path,
    seen_shim_metadata: &mut HashSet<String>,
) -> io::Result<std::process::ExitStatus> {
    loop {
        ingest_live_shim_identities(
            process_tree.registry().as_ref(),
            output_dir,
            seen_shim_metadata,
        );
        if let Some(status) = child.try_wait()? {
            if process_tree.interrupted() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "batch interrupted",
                ));
            }
            return Ok(status);
        }
        if process_tree.interrupted() {
            ingest_live_shim_identities(
                process_tree.registry().as_ref(),
                output_dir,
                seen_shim_metadata,
            );
            let _ = process_tree.terminate_descendants(Duration::from_millis(250));
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "batch interrupted",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn ingest_live_shim_identities(
    registry: &crate::batch_process_tree::ProcessTreeRegistry,
    output_dir: &Path,
    seen: &mut HashSet<String>,
) {
    let Ok(identities) = load_live_shim_process_identities(output_dir) else {
        return;
    };
    for identity in identities {
        let key = format!("{}:{}", identity.pid, identity.pgid);
        if seen.insert(key) {
            registry.record(identity);
        }
    }
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::process::{Command, Stdio};

    use crate::batch_process_tree::{BatchProcessTreeGuard, record_child_process_group};
    use crate::batch_shim::write_shim_start_metadata;

    use super::{ingest_live_shim_identities, wait_child_with_interruption};

    #[test]
    fn ingest_live_shim_identities_records_each_identity_once() {
        let guard = BatchProcessTreeGuard::install().expect("install guard");
        let mut children = Vec::new();
        let mut identities = Vec::new();
        for _ in 0..2 {
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg("sleep 0.5");
            command.stdin(Stdio::null());
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
            let child = guard
                .spawn_batch_command(&mut command)
                .expect("spawn child");
            record_child_process_group(guard.registry().as_ref(), &child);
            identities.push(
                guard
                    .registry()
                    .identities()
                    .last()
                    .expect("recorded child identity")
                    .clone(),
            );
            children.push(child);
        }
        let registry = crate::batch_process_tree::ProcessTreeRegistry::default();
        let tmp = tempfile::tempdir().unwrap();
        write_shim_start_metadata(tmp.path(), "alpha", &identities[0]).unwrap();
        write_shim_start_metadata(tmp.path(), "beta", &identities[1]).unwrap();
        let mut seen = HashSet::new();
        ingest_live_shim_identities(&registry, tmp.path(), &mut seen);
        ingest_live_shim_identities(&registry, tmp.path(), &mut seen);
        let recorded = registry.identities();
        assert_eq!(recorded.len(), 2);
        assert!(recorded.contains(&identities[0]));
        assert!(recorded.contains(&identities[1]));
        for mut child in children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[test]
    fn wait_child_with_interruption_returns_on_normal_exit() {
        let guard = BatchProcessTreeGuard::install().expect("install guard");
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 0.05");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let mut child = guard
            .spawn_batch_command(&mut command)
            .expect("spawn child");
        record_child_process_group(guard.registry().as_ref(), &child);
        let mut seen = HashSet::new();
        let status = wait_child_with_interruption(
            &mut child,
            &guard,
            std::path::Path::new("/nonexistent"),
            &mut seen,
        )
        .expect("wait child");
        assert!(status.success());
        assert!(!guard.interrupted());
    }

    #[test]
    fn wait_child_with_interruption_fails_when_interrupted() {
        let guard = BatchProcessTreeGuard::install().expect("install guard");
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 2");
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        let mut child = guard
            .spawn_batch_command(&mut command)
            .expect("spawn child");
        record_child_process_group(guard.registry().as_ref(), &child);
        guard.set_interrupted_for_test(true);
        let mut seen = HashSet::new();
        let err = wait_child_with_interruption(
            &mut child,
            &guard,
            std::path::Path::new("/nonexistent"),
            &mut seen,
        )
        .expect_err("expected interrupted wait");
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
        assert!(err.to_string().contains("batch interrupted"));
    }
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
