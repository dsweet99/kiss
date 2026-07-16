use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;
#[cfg(unix)]
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::batch_output_channel::{
    OutputChannelClient, OutputStreamKind, output_channel_config_from_env,
};
use crate::batch_process_tree::{ProcessGroupIdentity, signal_validated_process_group};
use crate::batch_runner_resolve::read_runner_map;

use super::BatchShimMetadata;
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
    #[cfg(unix)]
    {
        if unsafe { libc::setpgid(0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let shim_identity = current_process_group_identity()
        .ok_or_else(|| io::Error::other("failed to resolve shim process group identity"))?;
    let full_name = instance_full_name(command);
    let id = filesystem_safe_instance_id(&full_name);
    let profile_path = output_dir.join(format!("{id}.profraw"));
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

fn is_nextest_list_phase() -> bool {
    std::env::var("NEXTEST_TEST_PHASE").ok().as_deref() == Some("list")
}

fn run_delegated_child(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    instance_id: &str,
    output_dir: &Path,
) -> io::Result<DelegatedChildOutcome> {
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
        crate::batch_shim_delegated::release_delegated_child_handshake(&go_path)?;
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

fn delegated_child_spawn(
    delegated: &[String],
    command: &[OsString],
    profile_path: &Path,
    go_path: &Path,
) -> io::Result<(std::process::Child, Option<ProcessGroupIdentity>)> {
    #[cfg(unix)]
    {
        crate::batch_shim_delegated::spawn_delegated_piped_child(
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

#[cfg(unix)]
static SHIM_DELEGATED_PGID: AtomicU32 = AtomicU32::new(0);

pub(crate) struct ShimSignalForwarder;

impl ShimSignalForwarder {
    pub(crate) fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            install_shim_signal_forwarder()?;
        }
        Ok(Self)
    }

    pub(crate) fn set_delegated_identity(identity: &ProcessGroupIdentity) {
        #[cfg(unix)]
        {
            SHIM_DELEGATED_PGID.store(identity.pgid, Ordering::SeqCst);
        }
        #[cfg(not(unix))]
        {
            let _ = identity;
        }
    }

    pub(crate) fn clear_delegated_identity() {
        #[cfg(unix)]
        {
            SHIM_DELEGATED_PGID.store(0, Ordering::SeqCst);
        }
    }
}

impl Drop for ShimSignalForwarder {
    fn drop(&mut self) {
        Self::clear_delegated_identity();
        #[cfg(unix)]
        {
            clear_shim_signal_forwarder();
        }
    }
}

#[cfg(unix)]
static SHIM_SIGNAL_STATE: OnceLock<std::sync::Mutex<bool>> = OnceLock::new();

#[cfg(test)]
#[cfg(unix)]
pub(crate) fn trigger_shim_forward_signal_for_test(signal: libc::c_int) {
    shim_forward_signal(signal);
}

#[cfg(unix)]
extern "C" fn shim_forward_signal(signal: libc::c_int) {
    let pgid = SHIM_DELEGATED_PGID.load(Ordering::SeqCst);
    if pgid == 0 {
        return;
    }
    let identity = ProcessGroupIdentity { pid: pgid, pgid };
    signal_validated_process_group(&identity, signal);
}

#[cfg(unix)]
pub(crate) fn install_shim_signal_forwarder() -> io::Result<()> {
    let slot = SHIM_SIGNAL_STATE.get_or_init(|| std::sync::Mutex::new(false));
    *slot
        .lock()
        .map_err(|_| io::Error::other("shim signal forwarder lock poisoned"))? = true;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let previous = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = shim_forward_signal as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            let mut old = std::mem::zeroed();
            let rc = libc::sigaction(signal, &action, &mut old);
            if rc != 0 {
                return Err(io::Error::last_os_error());
            }
            old.sa_sigaction
        };
        let _ = previous;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn clear_shim_signal_forwarder() {
    if SHIM_SIGNAL_STATE.get().is_some() {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
        if let Ok(mut slot) = SHIM_SIGNAL_STATE.get().expect("shim signal state").lock() {
            *slot = false;
        }
    }
}
