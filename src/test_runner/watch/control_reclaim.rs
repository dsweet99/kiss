use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::Instant;

use super::super::lock::{WatchLockGuard, watch_lock_path};
use super::{CLIENT_SESSION_RETRY, CLIENT_SESSION_SLEEP, SessionFile, WATCH_SOCKET_TMP_DIR};

pub(crate) fn session_pid_is_live(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub(crate) fn reclaim_stale_watch_session(repo_root: &Path) {
    if let Ok(Some(session)) = super::read_session_file(repo_root) {
        reclaim_dead_session_socket(&session);
    }
    let _ = std::fs::remove_file(super::session_file_path(repo_root));
}

fn reclaim_dead_session_socket(session: &SessionFile) {
    let socket = Path::new(&session.socket);
    if !watch_socket_is_live(socket) {
        let _ = std::fs::remove_file(socket);
    }
}

pub(crate) enum SessionLook {
    Live(super::SessionFile),
    Reclaimed,
    Missing,
}

pub(crate) fn probe_live_watcher(repo_root: &Path) -> Result<Option<SessionFile>, String> {
    let lock_path = watch_lock_path(repo_root);
    match WatchLockGuard::try_lock_shared(&lock_path) {
        Ok(Some(_guard)) => {
            reclaim_stale_watch_session(repo_root);
            Ok(None)
        }
        Ok(None) => match take_live_session(repo_root)? {
            SessionLook::Live(session) => Ok(Some(session)),
            SessionLook::Reclaimed => Ok(None),
            SessionLook::Missing => Ok(Some(wait_for_session(repo_root)?)),
        },
        Err(e) => Err(format!("cannot probe watch lock: {e}")),
    }
}

fn wait_for_session(repo_root: &Path) -> Result<SessionFile, String> {
    let deadline = Instant::now() + CLIENT_SESSION_RETRY;
    loop {
        match super::read_session_file(repo_root)? {
            Some(session) if session_pid_is_live(session.pid) => return Ok(session),
            Some(_) => reclaim_stale_watch_session(repo_root),
            None => {}
        }
        if Instant::now() >= deadline {
            return Err("watcher lock held but session is not ready; try again shortly".into());
        }
        thread::sleep(CLIENT_SESSION_SLEEP);
    }
}

pub(crate) fn take_live_session(repo_root: &Path) -> Result<SessionLook, String> {
    match super::read_session_file(repo_root)? {
        Some(session) if session_pid_is_live(session.pid) => Ok(SessionLook::Live(session)),
        Some(_) => {
            reclaim_stale_watch_session(repo_root);
            Ok(SessionLook::Reclaimed)
        }
        None => Ok(SessionLook::Missing),
    }
}

pub(crate) fn reclaim_stale_watch_sockets(keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(WATCH_SOCKET_TMP_DIR) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sock") {
            continue;
        }
        if keep.is_some_and(|keep| keep == path.as_path()) {
            continue;
        }
        if watch_socket_is_live(&path) {
            continue;
        }
        let _ = std::fs::remove_file(&path);
    }
}

fn watch_socket_is_live(path: &Path) -> bool {
    match UnixStream::connect(path) {
        Ok(stream) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            true
        }
        Err(err) => !matches!(
            err.kind(),
            io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotFound
                | io::ErrorKind::PermissionDenied
        ),
    }
}
