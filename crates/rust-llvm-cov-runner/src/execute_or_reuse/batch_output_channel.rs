use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

pub const OUTPUT_CHANNEL_SOCKET_ENV: &str = "KISS_OUTPUT_CHANNEL_SOCKET";
pub const OUTPUT_CHANNEL_TOKEN_ENV: &str = "KISS_OUTPUT_CHANNEL_TOKEN";

pub(crate) const FRAME_MAGIC: &[u8; 4] = b"KOC1";
pub(crate) use crate::execute_or_reuse::batch_output_channel_frame::{read_frame, write_frame};
use crate::execute_or_reuse::batch_output_channel_token::{
    TOKEN_LEN, decode_token_hex, encode_token_hex, random_token,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStreamKind {
    Stdout = 0,
    Stderr = 1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChannelFrame {
    pub instance_id: String,
    pub sequence: u32,
    pub stream: OutputStreamKind,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChannelConfig {
    pub socket_path: PathBuf,
    pub token: [u8; TOKEN_LEN],
    pub relay_live: bool,
}

pub fn output_channel_config_from_env() -> Option<OutputChannelConfig> {
    let socket_path = std::env::var_os(OUTPUT_CHANNEL_SOCKET_ENV)?;
    let token_hex = std::env::var(OUTPUT_CHANNEL_TOKEN_ENV).ok()?;
    let token = decode_token_hex(&token_hex)?;
    Some(OutputChannelConfig {
        socket_path: PathBuf::from(socket_path),
        token,
        relay_live: false,
    })
}

pub(crate) const OUTPUT_CHANNEL_TMP_DIR: &str = "/tmp/.kiss-oc";

fn short_socket_path(run_root: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    run_root.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let digest = hasher.finish();
    PathBuf::from(format!("{OUTPUT_CHANNEL_TMP_DIR}/{digest:016x}.sock"))
}

pub fn create_output_channel_config(
    run_root: &Path,
    relay_live: bool,
) -> io::Result<OutputChannelConfig> {
    let socket_path = short_socket_path(run_root);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    Ok(OutputChannelConfig {
        socket_path,
        token: random_token(),
        relay_live,
    })
}

pub fn apply_output_channel_env(
    env: &mut std::collections::BTreeMap<String, String>,
    config: &OutputChannelConfig,
) {
    env.insert(
        OUTPUT_CHANNEL_SOCKET_ENV.to_string(),
        config.socket_path.to_string_lossy().to_string(),
    );
    env.insert(
        OUTPUT_CHANNEL_TOKEN_ENV.to_string(),
        encode_token_hex(&config.token),
    );
}

pub struct OutputChannelServer {
    config: OutputChannelConfig,
    frames: Arc<Mutex<Vec<OutputChannelFrame>>>,
    errors: Arc<Mutex<Vec<String>>>,
    accept_handle: Option<JoinHandle<()>>,
    connection_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    shutdown: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChannelStop {
    pub frames: Vec<OutputChannelFrame>,
    pub errors: Vec<String>,
}

impl OutputChannelServer {
    pub fn start(config: OutputChannelConfig) -> io::Result<Self> {
        if let Some(parent) = config.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(&config.socket_path)?;
        listener.set_nonblocking(true)?;
        let frames = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let connection_handles = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let accept_frames = Arc::clone(&frames);
        let accept_errors = Arc::clone(&errors);
        let accept_connection_handles = Arc::clone(&connection_handles);
        let accept_config = config.clone();
        let accept_shutdown = Arc::clone(&shutdown);
        let accept_handle = thread::spawn(move || {
            accept_connections(
                listener,
                accept_config,
                accept_frames,
                accept_errors,
                accept_connection_handles,
                accept_shutdown,
            )
        });
        Ok(Self {
            config,
            frames,
            errors,
            accept_handle: Some(accept_handle),
            connection_handles,
            shutdown,
        })
    }

    #[cfg(test)]
    pub fn stop(self) -> Vec<OutputChannelFrame> {
        self.stop_with_errors().frames
    }

    pub fn stop_with_errors(mut self) -> OutputChannelStop {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept_handle.take() {
            let _ = handle.join();
        }
        let handles = self
            .connection_handles
            .lock()
            .expect("output channel connection handles lock")
            .drain(..)
            .collect::<Vec<_>>();
        for handle in handles {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.config.socket_path);
        let frames = self
            .frames
            .lock()
            .expect("output channel frames lock")
            .clone();
        let errors = self
            .errors
            .lock()
            .expect("output channel errors lock")
            .clone();
        OutputChannelStop { frames, errors }
    }
}

#[cfg(test)]
pub fn send_output_frames(
    config: &OutputChannelConfig,
    instance_id: &str,
    stdout: &[u8],
    stderr: &[u8],
) -> io::Result<()> {
    let mut client = OutputChannelClient::connect(config)?;
    for chunk in chunk_output(stdout) {
        client.send_chunk(instance_id, OutputStreamKind::Stdout, chunk)?;
    }
    for chunk in chunk_output(stderr) {
        client.send_chunk(instance_id, OutputStreamKind::Stderr, chunk)?;
    }
    client.shutdown()
}

pub struct OutputChannelClient {
    stream: UnixStream,
    token: [u8; TOKEN_LEN],
    sequence: u32,
}

impl OutputChannelClient {
    pub fn connect(config: &OutputChannelConfig) -> io::Result<Self> {
        Ok(Self {
            stream: UnixStream::connect(&config.socket_path)?,
            token: config.token,
            sequence: 0,
        })
    }

    pub fn send_chunk(
        &mut self,
        instance_id: &str,
        stream_kind: OutputStreamKind,
        payload: &[u8],
    ) -> io::Result<()> {
        write_frame(
            &mut self.stream,
            &self.token,
            instance_id,
            self.sequence,
            stream_kind,
            payload,
        )?;
        self.sequence += 1;
        Ok(())
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Both)
    }
}

fn accept_connections(
    listener: UnixListener,
    config: OutputChannelConfig,
    frames: Arc<Mutex<Vec<OutputChannelFrame>>>,
    errors: Arc<Mutex<Vec<String>>>,
    connection_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let config = config.clone();
                let frames = Arc::clone(&frames);
                let errors = Arc::clone(&errors);
                let handle = thread::spawn(move || {
                    if let Err(err) = read_connection_frames(&mut stream, &config, &frames) {
                        errors
                            .lock()
                            .expect("output channel errors lock")
                            .push(err.to_string());
                    }
                });
                connection_handles
                    .lock()
                    .expect("output channel connection handles lock")
                    .push(handle);
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(err) => {
                errors
                    .lock()
                    .expect("output channel errors lock")
                    .push(err.to_string());
                break;
            }
        }
    }
}

pub(crate) fn read_connection_frames(
    stream: &mut UnixStream,
    config: &OutputChannelConfig,
    frames: &Arc<Mutex<Vec<OutputChannelFrame>>>,
) -> io::Result<()> {
    loop {
        let frame = match read_frame(stream, &config.token) {
            Ok(frame) => frame,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(err) => return Err(err),
        };
        if config.relay_live {
            crate::execute_or_reuse::batch_output_channel_frame::relay_frame_live(&frame);
        }
        frames
            .lock()
            .expect("output channel frames lock")
            .push(frame);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn chunk_output(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    const CHUNK_SIZE: usize = 4096;
    let mut offset = 0;
    std::iter::from_fn(move || {
        if offset >= bytes.len() {
            return None;
        }
        let end = (offset + CHUNK_SIZE).min(bytes.len());
        let chunk = &bytes[offset..end];
        offset = end;
        Some(chunk)
    })
}

#[cfg(test)]
impl OutputStreamKind {
    pub(crate) fn witness_values() -> [Self; 2] {
        [Self::Stdout, Self::Stderr]
    }
}

#[cfg(test)]
impl OutputChannelConfig {
    pub(crate) fn witness() -> Self {
        create_output_channel_config(Path::new("/tmp"), false)
            .expect("witness output channel config")
    }
}

#[cfg(test)]
impl OutputChannelServer {
    pub(crate) fn witness_frames(&self) -> usize {
        self.frames.lock().expect("frames lock").len()
    }
}

#[cfg(test)]
mod inline_output_channel_coverage {
    use super::*;

    #[test]
    fn output_channel_stop_debug_includes_frame_and_error_counts() {
        let stop = OutputChannelStop {
            frames: vec![OutputChannelFrame {
                instance_id: "id".to_string(),
                sequence: 0,
                stream: OutputStreamKind::Stdout,
                bytes: b"x".to_vec(),
            }],
            errors: vec!["accept failed".to_string()],
        };
        let debug = format!("{stop:?}");
        assert!(debug.contains("frames"));
        assert!(debug.contains("errors"));
    }
}

#[cfg(test)]
#[path = "batch_output_channel_extra_test.rs"]
mod extra_tests;

#[cfg(test)]
#[path = "batch_output_channel_test.rs"]
mod tests;
