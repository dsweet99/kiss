use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::lock::{watch_dir, watch_lock_path, WatchLockGuard};

pub(crate) const WATCH_SOCKET_TMP_DIR: &str = "/tmp/.kiss-watch";

const SESSION_FILE_NAME: &str = "session.json";
const MAX_FRAME_LEN: u32 = 256 * 1024;
const CLIENT_SESSION_RETRY: Duration = Duration::from_millis(500);
const CLIENT_SESSION_SLEEP: Duration = Duration::from_millis(10);
const REPLY_IMMEDIATE_WAIT: Duration = Duration::from_millis(250);

pub(crate) use super::nudge_kind::NudgeInvocation;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub(crate) struct NudgeRequestMsg {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub force_bad: bool,
    #[serde(default)]
    pub metrics: bool,
    #[serde(default)]
    pub invocation: NudgeInvocation,
    #[serde(default)]
    pub targets: Vec<String>,
}

impl NudgeRequestMsg {
    pub(crate) fn progress_line(&self) -> String {
        let mut line = format!(
            "kiss test: request force={} force_bad={} metrics={}",
            self.force, self.force_bad, self.metrics
        );
        if !self.invocation.is_all() {
            line.push_str(" invocation=");
            line.push_str(self.invocation.as_str());
        }
        if !self.targets.is_empty() {
            line.push_str(" targets=");
            line.push_str(&self.targets.join(" "));
        }
        line
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct NudgeReplyMsg {
    pub exit_code: i32,
    pub pid: u32,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SessionFile {
    pub pid: u32,
    pub socket: String,
}

pub(crate) struct NudgeRequest {
    pub msg: NudgeRequestMsg,
    pub reply: SyncSender<NudgeReplyMsg>,
}

pub(crate) struct WatchControlServer {
    socket_path: PathBuf,
    session_path: PathBuf,
    _listener_keep: (),
    shutdown: Arc<AtomicBool>,
    accept_handle: Option<JoinHandle<()>>,
    pub nudge_rx: Receiver<NudgeRequest>,
}

impl WatchControlServer {
    pub(crate) fn start(repo_root: &Path) -> Result<Self, String> {
        let socket_path = watch_socket_path(repo_root)?;
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        if socket_path.exists() {
            let _ = std::fs::remove_file(&socket_path);
        }
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("cannot bind {}: {e}", socket_path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("cannot set nonblocking: {e}"))?;

        let session_path = session_file_path(repo_root);
        write_session_file(
            &session_path,
            &SessionFile {
                pid: std::process::id(),
                socket: socket_path.to_string_lossy().into_owned(),
            },
        )?;

        let (nudge_tx, nudge_rx) = mpsc::channel::<NudgeRequest>();
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_accept = Arc::clone(&shutdown);
        let accept_handle = thread::spawn(move || {
            accept_loop(listener, nudge_tx, shutdown_accept);
        });

        Ok(Self {
            socket_path,
            session_path,
            _listener_keep: (),
            shutdown,
            accept_handle: Some(accept_handle),
            nudge_rx,
        })
    }
}

impl Drop for WatchControlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(&self.session_path);
        if let Some(handle) = self.accept_handle.take() {
            let _ = handle.join();
        }
    }
}

pub(crate) struct WatchSessionOwner {
    pub _lock: WatchLockGuard,
    pub control: WatchControlServer,
}

impl WatchSessionOwner {
    pub(crate) fn acquire(repo_root: &Path) -> Result<Self, String> {
        let lock = acquire_exclusive_watch_lock(repo_root)?;
        let control = WatchControlServer::start(repo_root)?;
        Ok(Self {
            _lock: lock,
            control,
        })
    }
}

fn acquire_exclusive_watch_lock(repo_root: &Path) -> Result<WatchLockGuard, String> {
    let lock_path = watch_lock_path(repo_root);
    let deadline = Instant::now() + CLIENT_SESSION_RETRY;
    loop {
        match WatchLockGuard::try_lock(&lock_path) {
            Ok(Some(guard)) => return Ok(guard),
            Ok(None) => {
                if let Ok(Some(session)) = read_session_file(repo_root) {
                    return Err(format!("watcher already running (pid {})", session.pid));
                }
                if Instant::now() >= deadline {
                    return Err("watcher already running".into());
                }
                thread::sleep(CLIENT_SESSION_SLEEP);
            }
            Err(e) => return Err(format!("cannot lock {}: {e}", lock_path.display())),
        }
    }
}

pub(crate) fn probe_live_watcher(repo_root: &Path) -> Result<Option<SessionFile>, String> {
    let lock_path = watch_lock_path(repo_root);
    match WatchLockGuard::try_lock_shared(&lock_path) {
        Ok(Some(_guard)) => Ok(None),
        Ok(None) => Ok(Some(wait_for_session(repo_root)?)),
        Err(e) => Err(format!("cannot probe watch lock: {e}")),
    }
}

#[cfg(test)]
pub(crate) fn try_client_nudge(
    repo_root: &Path,
    msg: &NudgeRequestMsg,
) -> Result<Option<NudgeReplyMsg>, String> {
    match probe_live_watcher(repo_root)? {
        None => Ok(None),
        Some(session) => Ok(Some(nudge_watcher_with_retry_on_wait(
            repo_root,
            &session,
            msg,
            &mut || {},
        )?)),
    }
}

fn wait_for_session(repo_root: &Path) -> Result<SessionFile, String> {
    let deadline = Instant::now() + CLIENT_SESSION_RETRY;
    loop {
        match read_session_file(repo_root)? {
            Some(session) => return Ok(session),
            None if Instant::now() < deadline => {
                thread::sleep(CLIENT_SESSION_SLEEP);
            }
            None => {
                return Err("watcher lock held but session is not ready; try again shortly".into());
            }
        }
    }
}

pub(crate) fn nudge_watcher_with_retry_on_wait(
    repo_root: &Path,
    session: &SessionFile,
    msg: &NudgeRequestMsg,
    on_slow: &mut dyn FnMut(),
) -> Result<NudgeReplyMsg, String> {
    let deadline = Instant::now() + CLIENT_SESSION_RETRY;
    let mut current = session.clone();
    loop {
        match nudge_watcher_on_wait(&current, msg, on_slow) {
            Ok(reply) => return Ok(reply),
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                if let Ok(Some(updated)) = read_session_file(repo_root) {
                    current = updated;
                }
                thread::sleep(CLIENT_SESSION_SLEEP);
            }
            Err(e) => return Err(e),
        }
    }
}

fn nudge_watcher_on_wait(
    session: &SessionFile,
    msg: &NudgeRequestMsg,
    on_slow: &mut dyn FnMut(),
) -> Result<NudgeReplyMsg, String> {
    let mut stream = UnixStream::connect(&session.socket)
        .map_err(|e| format!("cannot connect to watcher socket: {e}"))?;
    write_framed_json(&mut stream, msg).map_err(|e| format!("nudge write failed: {e}"))?;
    if !socket_readable_within(&stream, REPLY_IMMEDIATE_WAIT) {
        on_slow();
    }
    let reply: NudgeReplyMsg =
        read_framed_json(&mut stream).map_err(|e| format!("nudge read failed: {e}"))?;
    Ok(reply)
}

fn socket_readable_within(stream: &UnixStream, timeout: Duration) -> bool {
    use std::os::fd::AsRawFd;

    let mut pfd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    loop {
        let n = unsafe { libc::poll(&mut pfd, 1, ms) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return false;
        }
        return n > 0;
    }
}

pub(crate) fn session_file_path(repo_root: &Path) -> PathBuf {
    watch_dir(repo_root).join(SESSION_FILE_NAME)
}

pub(crate) fn read_session_file(repo_root: &Path) -> Result<Option<SessionFile>, String> {
    let path = session_file_path(repo_root);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let session: SessionFile = serde_json::from_slice(&bytes)
        .map_err(|e| format!("cannot parse {}: {e}", path.display()))?;
    Ok(Some(session))
}

pub(crate) fn write_session_file(path: &Path, session: &SessionFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(session).map_err(|e| format!("session encode: {e}"))?;
    std::fs::write(&tmp, &bytes).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot publish session: {e}"))?;
    Ok(())
}

pub(crate) fn watch_socket_path(repo_root: &Path) -> Result<PathBuf, String> {
    let canonical = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let digest = hasher.finish();
    Ok(PathBuf::from(format!(
        "{WATCH_SOCKET_TMP_DIR}/{digest:016x}.sock"
    )))
}

fn accept_loop(listener: UnixListener, nudge_tx: Sender<NudgeRequest>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let tx = nudge_tx.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, tx) {
                        eprintln!("kiss test --watch: control client error: {e}");
                    }
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) if shutdown.load(Ordering::SeqCst) => break,
            Err(e) => {
                eprintln!("kiss test --watch: accept error: {e}");
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn handle_client(mut stream: UnixStream, nudge_tx: Sender<NudgeRequest>) -> Result<(), String> {
    let msg: NudgeRequestMsg = read_framed_json(&mut stream).map_err(|e| e.to_string())?;
    crate::test_runner::emit_test_progress(&msg.progress_line());
    let (reply_tx, reply_rx) = mpsc::sync_channel::<NudgeReplyMsg>(1);
    nudge_tx
        .send(NudgeRequest {
            msg,
            reply: reply_tx,
        })
        .map_err(|_| "watcher nudge channel closed".to_string())?;
    let reply = reply_rx
        .recv()
        .map_err(|_| "watcher closed before reply".to_string())?;
    write_framed_json(&mut stream, &reply).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn write_framed_json<T: Serialize>(
    stream: &mut UnixStream,
    value: &T,
) -> io::Result<()> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME_LEN as usize {
        return Err(io::Error::other("frame too large"));
    }
    let len = (body.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

pub(crate) fn read_framed_json<T: for<'de> Deserialize<'de>>(
    stream: &mut UnixStream,
) -> io::Result<T> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_LEN {
        return Err(io::Error::other("frame too large"));
    }
    let mut body = vec![0u8; len as usize];
    stream.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
#[path = "control_test.rs"]
mod tests;
