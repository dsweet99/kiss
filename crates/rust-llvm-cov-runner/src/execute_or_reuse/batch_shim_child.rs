use std::ffi::OsString;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use crate::execute_or_reuse::batch_output_channel::{
    OutputChannelClient, OutputStreamKind, output_channel_config_from_env,
};
use crate::execute_or_reuse::batch_process_tree::ProcessGroupIdentity;
use crate::plan::batch_runner_resolve::read_runner_map;

use super::BatchShimMetadata;
use super::batch_shim_signal::ShimSignalForwarder;
use super::batch_shim_write::{filesystem_safe_instance_id, instance_full_name};
use super::batch_shim_write::{
    write_delegated_start_metadata, write_shim_metadata, write_shim_start_metadata,
};

pub(crate) type DelegatedChildOutcome = (
    Option<i32>,
    Option<String>,
    Option<ProcessGroupIdentity>,
    Vec<u8>,
    Vec<u8>,
    Option<u64>,
);

pub(crate) fn run_target_runner_shim_inner(
    output_dir: &Path,
    runner_map: &Path,
    platform: &str,
    command: &[OsString],
) -> io::Result<i32> {
    crate::kiss_profraw::redirect_inherited_llvm_profile_file(output_dir)?;
    let result = run_target_runner_shim_after_redirect(output_dir, runner_map, platform, command);
    let cleanup_err = crate::kiss_profraw::cleanup_kiss_profraw_for_pid(
        &crate::kiss_profraw::resolve_kiss_profraw(output_dir),
        std::process::id(),
    )
    .err();
    match (result, cleanup_err) {
        (Ok(code), None) => Ok(code),
        (Ok(_), Some(err)) => Err(err),
        (Err(err), None) => Err(err),
        (Err(err), Some(cleanup)) => Err(io::Error::new(err.kind(), format!("{err}; {cleanup}"))),
    }
}

fn run_target_runner_shim_after_redirect(
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
        return super::batch_shim_list::run_delegated_list_child(output_dir, &delegated, command);
    }
    std::fs::create_dir_all(output_dir)?;
    set_current_process_group()?;
    let shim_identity = current_process_group_identity()
        .ok_or_else(|| io::Error::other("failed to resolve shim process group identity"))?;
    let full_name = instance_full_name(command);
    let id = filesystem_safe_instance_id(&full_name);
    let profile_path = profile_path_for_instance(output_dir, &id, command);
    if let Some(parent) = profile_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _signal_guard = ShimSignalForwarder::install()?;
    write_shim_start_metadata(output_dir, &id, &shim_identity)?;
    let (exit_code, spawn_error, delegated_identity, stdout, stderr, output_frame_count) =
        run_delegated_child(&delegated, command, &profile_path, &id, output_dir)?;
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
        shim_identity: Some(shim_identity),
        delegated_identity,
        stdout: Some(stdout),
        stderr: Some(stderr),
        output_frame_count,
    };
    write_shim_metadata(output_dir, &id, &metadata)?;
    Ok(exit_code.unwrap_or(1))
}

pub(crate) const PROFILE_POOL_ENV: &str = "KISS_RUST_COVERAGE_PROFILE_POOL";
pub(crate) const PROFILE_POOL_FILE_PATTERN: &str = "pool-%32m.profraw";

pub(crate) fn profile_path_for_instance(
    output_dir: &Path,
    instance_id: &str,
    command: &[OsString],
) -> PathBuf {
    if std::env::var(PROFILE_POOL_ENV).ok().as_deref() == Some("1") {
        let binary = command
            .first()
            .map(|arg| arg.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown-binary".to_string());
        let pool_key = filesystem_safe_instance_id(&binary);
        output_dir.join(pool_key).join(PROFILE_POOL_FILE_PATTERN)
    } else {
        output_dir.join(format!("{instance_id}.profraw"))
    }
}

pub(super) fn is_nextest_list_phase() -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("list")
}

fn run_delegated_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    instance_id: &str,
    output_dir: &Path,
) -> io::Result<DelegatedChildOutcome> {
    if std::env::var(PROFILE_POOL_ENV).ok().as_deref() == Some("1") {
        return run_delegated_child_pool_fast(delegated, command, profile_path);
    }
    let go_path = output_dir.join(format!("{instance_id}.delegated-go"));
    let spawn = match delegated_child_spawn(delegated, command, profile_path, &go_path) {
        Ok(spawn) => spawn,
        Err(err) => {
            return Ok((
                Some(1),
                Some(err.to_string()),
                None,
                Vec::new(),
                Vec::new(),
                None,
            ));
        }
    };
    let (mut child, delegated_identity) = spawn;
    if let Some(identity) = delegated_identity.as_ref() {
        write_delegated_start_metadata(output_dir, instance_id, identity)?;
        ShimSignalForwarder::set_delegated_identity(identity);
    }
    #[cfg(unix)]
    {
        crate::execute_or_reuse::batch_shim_delegated::release_delegated_child_handshake(&go_path)?;
    }
    let (stdout, stderr, output_frame_count) =
        collect_delegated_child_output(&mut child, instance_id)?;
    let status = child.wait()?;
    ShimSignalForwarder::clear_delegated_identity();
    Ok((
        status.code().or(Some(1)),
        None,
        delegated_identity,
        stdout,
        stderr,
        output_frame_count,
    ))
}

fn run_delegated_child_pool_fast(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
) -> io::Result<DelegatedChildOutcome> {
    let mut child = build_delegated_command(delegated, command);
    crate::execute_or_reuse::batch_shim_delegated::scrub_coverage_build_env(&mut child);
    child.env("LLVM_PROFILE_FILE", profile_path);
    child.stdout(Stdio::null());
    child.stderr(Stdio::null());
    child.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            child.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut spawned = match child.spawn() {
        Ok(spawned) => spawned,
        Err(err) => {
            return Ok((
                Some(1),
                Some(err.to_string()),
                None,
                Vec::new(),
                Vec::new(),
                None,
            ));
        }
    };
    let status = spawned.wait()?;
    Ok((
        status.code().or(Some(1)),
        None,
        None,
        Vec::new(),
        Vec::new(),
        None,
    ))
}

fn delegated_child_spawn(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    go_path: &Path,
) -> io::Result<(std::process::Child, Option<ProcessGroupIdentity>)> {
    #[cfg(unix)]
    {
        crate::execute_or_reuse::batch_shim_delegated::spawn_delegated_piped_child(
            delegated,
            command,
            profile_path,
            go_path,
        )
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
fn set_current_process_group() -> io::Result<()> {
    if unsafe { libc::setpgid(0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_current_process_group() -> io::Result<()> {
    Ok(())
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

pub(crate) fn build_delegated_command(delegated: &[String], command: &[OsString]) -> Command {
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
) -> io::Result<(Vec<u8>, Vec<u8>, Option<u64>)> {
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child stdout pipe"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing child stderr pipe"))?;
    let shared_client = connect_shared_output_client()?;
    let frame_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let stdout_handle = std::thread::spawn({
        let instance_id = instance_id.to_string();
        let shared_client = shared_client.clone();
        let frame_counter = std::sync::Arc::clone(&frame_counter);
        move || {
            drain_child_stream(
                stdout_pipe,
                instance_id,
                &shared_client,
                OutputStreamKind::Stdout,
                Some(frame_counter),
            )
        }
    });
    let stderr_handle = std::thread::spawn({
        let instance_id = instance_id.to_string();
        let shared_client = shared_client.clone();
        let frame_counter = std::sync::Arc::clone(&frame_counter);
        move || {
            drain_child_stream(
                stderr_pipe,
                instance_id,
                &shared_client,
                OutputStreamKind::Stderr,
                Some(frame_counter),
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
    let output_frame_count = if shared_client.is_some() {
        Some(frame_counter.load(Ordering::SeqCst))
    } else {
        None
    };
    Ok((stdout, stderr, output_frame_count))
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
    frame_counter: Option<std::sync::Arc<std::sync::atomic::AtomicU64>>,
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
            if let Some(counter) = frame_counter.as_ref() {
                counter.fetch_add(1, Ordering::SeqCst);
            }
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

#[cfg(test)]
#[path = "batch_shim_child_profile_test.rs"]
mod profile_pool_tests;
