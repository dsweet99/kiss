use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde::{Deserialize, Serialize};

use crate::batch_output_channel::{
    OutputChannelClient, OutputStreamKind, output_channel_config_from_env,
};
use crate::batch_process_tree::ProcessGroupIdentity;
use crate::batch_runner_resolve::read_runner_map;

pub const TARGET_RUNNER_SHIM_SUBCOMMAND: &str = "__rust-llvm-cov-target-runner";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchShimMetadata {
    pub schema_version: String,
    pub id: String,
    pub full_name: String,
    pub profile_path: PathBuf,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub spawn_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shim_identity: Option<ProcessGroupIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_identity: Option<ProcessGroupIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<Vec<u8>>,
}

pub fn run_target_runner_shim(
    output_dir: &Path,
    runner_map: &Path,
    platform: &str,
    command: &[OsString],
) -> i32 {
    match run_target_runner_shim_inner(output_dir, runner_map, platform, command) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("kiss rust llvm-cov target runner: {err}");
            1
        }
    }
}

pub(crate) fn load_target_runner_shim_metadata(
    output_dir: &Path,
) -> io::Result<Vec<BatchShimMetadata>> {
    if !output_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut metadata = Vec::new();
    for entry in fs::read_dir(output_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(path)?;
        metadata.push(serde_json::from_slice(&bytes).map_err(io::Error::other)?);
    }
    metadata.sort_by(|left: &BatchShimMetadata, right| left.full_name.cmp(&right.full_name));
    Ok(metadata)
}

fn run_target_runner_shim_inner(
    output_dir: &Path,
    runner_map: &Path,
    platform: &str,
    command: &[OsString],
) -> io::Result<i32> {
    if command.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing test binary command",
        ));
    }
    let delegated = read_runner_map(runner_map)
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to read runner map: {err:?}"),
            )
        })?
        .get(platform)
        .cloned()
        .unwrap_or_default();
    if is_nextest_list_phase() {
        return run_delegated_list_child(&delegated, command);
    }
    fs::create_dir_all(output_dir)?;
    let shim_identity = current_process_group_identity();
    let full_name = instance_full_name(command);
    let id = filesystem_safe_instance_id(&full_name);
    let profile_path = output_dir.join(format!("{id}.profraw"));
    let (exit_code, spawn_error, delegated_identity, stdout, stderr) =
        run_delegated_child(&delegated, command, &profile_path, &id)?;
    let metadata = BatchShimMetadata {
        schema_version: "kiss-rust-llvm-cov-shim-v2".to_string(),
        id: id.clone(),
        full_name,
        profile_path,
        cwd: std::env::current_dir()?,
        argv: command
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect(),
        exit_code,
        spawn_error,
        shim_identity,
        delegated_identity,
        stdout: Some(stdout),
        stderr: Some(stderr),
    };
    write_shim_metadata(output_dir, &id, &metadata)?;
    Ok(exit_code.unwrap_or(1))
}

type DelegatedChildOutcome = (
    Option<i32>,
    Option<String>,
    Option<ProcessGroupIdentity>,
    Vec<u8>,
    Vec<u8>,
);

fn is_nextest_list_phase() -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("list")
}

fn run_delegated_list_child(delegated: &[String], command: &[OsString]) -> io::Result<i32> {
    let status = build_delegated_command(delegated, command)
        .stdin(Stdio::null())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

fn run_delegated_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    instance_id: &str,
) -> io::Result<DelegatedChildOutcome> {
    let (mut child, delegated_identity) =
        match spawn_delegated_piped_child(delegated, command, profile_path) {
            Ok(pair) => pair,
            Err(err) => {
                return Ok((
                    Some(1),
                    Some(err.to_string()),
                    None,
                    Vec::new(),
                    Vec::new(),
                ));
            }
        };
    let (stdout, stderr) = collect_delegated_child_output(&mut child, instance_id)?;
    let status = child.wait()?;
    Ok((
        status.code().or(Some(1)),
        None,
        delegated_identity,
        stdout,
        stderr,
    ))
}

fn spawn_delegated_piped_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
) -> io::Result<(std::process::Child, Option<ProcessGroupIdentity>)> {
    #[cfg(unix)]
    {
        let mut command = build_delegated_command(delegated, command);
        command.env("LLVM_PROFILE_FILE", profile_path);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdin(Stdio::null());
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let delegated_identity = record_spawned_child_identity(&child);
        Ok((child, delegated_identity))
    }
    #[cfg(not(unix))]
    {
        let mut child = build_delegated_command(delegated, command);
        child.env("LLVM_PROFILE_FILE", profile_path);
        child.stdout(Stdio::piped());
        child.stderr(Stdio::piped());
        child.stdin(Stdio::null());
        Ok((child.spawn()?, None))
    }
}

#[cfg(unix)]
fn record_spawned_child_identity(child: &std::process::Child) -> Option<ProcessGroupIdentity> {
    let pid = child.id();
    if pid == 0 {
        return None;
    }
    let pgid = unsafe { libc::getpgid(pid as i32) };
    if pgid <= 0 {
        return None;
    }
    Some(ProcessGroupIdentity {
        pid,
        pgid: pgid as u32,
    })
}

#[cfg(unix)]
fn current_process_group_identity() -> Option<ProcessGroupIdentity> {
    let pid = std::process::id();
    let pgid = unsafe { libc::getpgid(pid as i32) };
    if pgid <= 0 {
        return None;
    }
    Some(ProcessGroupIdentity {
        pid,
        pgid: pgid as u32,
    })
}

#[cfg(not(unix))]
fn current_process_group_identity() -> Option<ProcessGroupIdentity> {
    None
}

fn build_delegated_command(delegated: &[String], command: &[OsString]) -> Command {
    if delegated.is_empty() {
        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..]);
        cmd
    } else {
        let mut cmd = Command::new(&delegated[0]);
        cmd.args(&delegated[1..]);
        cmd.args(command);
        cmd
    }
}

fn collect_delegated_child_output(
    child: &mut std::process::Child,
    instance_id: &str,
) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child stdout pipe"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing child stderr pipe"))?;
    let shared_client = connect_shared_output_client()?;
    let stdout_handle = std::thread::spawn({
        let instance_id = instance_id.to_string();
        let shared_client = shared_client.clone();
        move || {
            drain_child_stream(
                stdout_pipe,
                instance_id,
                &shared_client,
                OutputStreamKind::Stdout,
            )
        }
    });
    let stderr_handle = std::thread::spawn({
        let instance_id = instance_id.to_string();
        let shared_client = shared_client.clone();
        move || {
            drain_child_stream(
                stderr_pipe,
                instance_id,
                &shared_client,
                OutputStreamKind::Stderr,
            )
        }
    });
    let stdout = stdout_handle
        .join()
        .map_err(|_| io::Error::other("stdout drain panicked"))??;
    let stderr = stderr_handle
        .join()
        .map_err(|_| io::Error::other("stderr drain panicked"))??;
    shutdown_shared_output_client(&shared_client)?;
    Ok((stdout, stderr))
}

fn connect_shared_output_client()
-> io::Result<Option<std::sync::Arc<std::sync::Mutex<OutputChannelClient>>>> {
    let Some(channel) = output_channel_config_from_env() else {
        return Ok(None);
    };
    match OutputChannelClient::connect(&channel) {
        Ok(client) => Ok(Some(std::sync::Arc::new(std::sync::Mutex::new(client)))),
        Err(err) => Err(err),
    }
}

fn drain_child_stream<R: Read>(
    mut pipe: R,
    instance_id: String,
    shared_client: &Option<std::sync::Arc<std::sync::Mutex<OutputChannelClient>>>,
    stream_kind: OutputStreamKind,
) -> io::Result<Vec<u8>> {
    let mut collected = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = pipe.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&chunk[..read]);
        if let Some(client) = shared_client.as_ref() {
            client
                .lock()
                .map_err(|_| io::Error::other("output channel client lock poisoned"))?
                .send_chunk(&instance_id, stream_kind, &chunk[..read])?;
        }
    }
    Ok(collected)
}

fn shutdown_shared_output_client(
    shared_client: &Option<std::sync::Arc<std::sync::Mutex<OutputChannelClient>>>,
) -> io::Result<()> {
    if let Some(client) = shared_client {
        client
            .lock()
            .map_err(|_| io::Error::other("output channel client lock poisoned"))?
            .shutdown()?;
    }
    Ok(())
}

pub(crate) fn write_shim_metadata(
    output_dir: &Path,
    id: &str,
    metadata: &BatchShimMetadata,
) -> io::Result<()> {
    let metadata_path = output_dir.join(format!("{id}.json"));
    let tmp_path = output_dir.join(format!(".{id}.json.tmp"));
    let mut file = create_new_file(&tmp_path)?;
    serde_json::to_writer(&mut file, metadata).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, metadata_path)
}

fn create_new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn instance_full_name(command: &[OsString]) -> String {
    if let Some((binary, test_name)) = exact_test_from_command(command) {
        let binary_id = binary
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| binary.to_string_lossy().to_string());
        return format!("{binary_id}${test_name}");
    }
    if should_use_nextest_env_for_instance(command)
        && let (Some(binary_id), Some(test_name)) = (
            std::env::var("NEXTEST_BINARY_ID").ok(),
            std::env::var("NEXTEST_TEST_NAME").ok(),
        )
    {
        return format!("{binary_id}${test_name}");
    }
    unique_shim_id()
}

fn should_use_nextest_env_for_instance(command: &[OsString]) -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("run")
        && command.first().is_some_and(|arg| {
            let path = arg.to_string_lossy();
            !path.ends_with(".sh") && !path.ends_with(".bat")
        })
}

fn filesystem_safe_instance_id(full_name: &str) -> String {
    full_name.replace(['/', '\\'], "_")
}

fn exact_test_from_command(command: &[OsString]) -> Option<(PathBuf, String)> {
    if command.len() < 3 {
        return None;
    }
    let binary = PathBuf::from(&command[0]);
    let args: Vec<_> = command[1..]
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();
    let exact_index = args.iter().position(|arg| arg == "--exact")?;
    let test_name = args.get(exact_index + 1)?.clone();
    Some((binary, test_name))
}

fn unique_shim_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", std::process::id(), nanos)
}

#[cfg(test)]
#[path = "batch_shim_test.rs"]
mod tests;
