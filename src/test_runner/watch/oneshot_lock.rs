use std::path::Path;
use std::thread;
use std::time::Duration;

use super::control::{read_session_file, session_pid_is_live};
use super::lock::{WatchLockGuard, watch_lock_path};

const ONESHOT_LOCK_POLL: Duration = Duration::from_millis(10);

pub(crate) enum OneshotPeer {
    Lock(WatchLockGuard),
    WatcherExit(i32),
}

pub(crate) fn wait_oneshot_peer(
    repo_root: &Path,
    mut try_watcher: impl FnMut() -> Result<Option<i32>, String>,
    mut on_wait: impl FnMut(),
) -> Result<OneshotPeer, String> {
    let lock_path = watch_lock_path(repo_root);
    loop {
        if let Some(peer) =
            oneshot_peer_step(repo_root, &lock_path, &mut try_watcher, &mut on_wait)?
        {
            return Ok(peer);
        }
    }
}

fn oneshot_peer_step(
    repo_root: &Path,
    lock_path: &Path,
    try_watcher: &mut impl FnMut() -> Result<Option<i32>, String>,
    on_wait: &mut impl FnMut(),
) -> Result<Option<OneshotPeer>, String> {
    if watcher_session_is_live(repo_root)
        && let Some(code) = try_watcher()?
    {
        return Ok(Some(OneshotPeer::WatcherExit(code)));
    }
    match WatchLockGuard::try_lock(lock_path) {
        Ok(Some(guard)) => Ok(Some(OneshotPeer::Lock(guard))),
        Ok(None) => {
            poll_oneshot_wait(on_wait);
            Ok(None)
        }
        Err(e) => Err(format!("cannot lock {}: {e}", lock_path.display())),
    }
}

fn poll_oneshot_wait(on_wait: &mut impl FnMut()) {
    on_wait();
    thread::sleep(ONESHOT_LOCK_POLL);
}

fn watcher_session_is_live(repo_root: &Path) -> bool {
    match read_session_file(repo_root) {
        Ok(Some(session)) => session_pid_is_live(session.pid),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn poll_oneshot_wait_invokes_callback() {
        let mut hits = 0usize;
        poll_oneshot_wait(&mut || hits += 1);
        assert_eq!(hits, 1);
    }

    #[test]
    fn oneshot_peer_step_wait_path_when_lock_held() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let path = watch_lock_path(repo);
        let _peer = WatchLockGuard::lock(&path).unwrap();
        let mut waited = false;
        let step = oneshot_peer_step(repo, &path, &mut || Ok(None), &mut || waited = true).unwrap();
        assert!(step.is_none());
        assert!(waited);
    }

    #[test]
    fn wait_oneshot_peer_blocks_until_peer_releases_tmp_repo_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().to_path_buf();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let path = watch_lock_path(&repo);
        let peer = WatchLockGuard::lock(&path).unwrap();
        let waited = std::sync::Arc::new(AtomicBool::new(false));
        let watcher_calls = std::sync::Arc::new(AtomicUsize::new(0));
        let flag = std::sync::Arc::clone(&waited);
        let calls = std::sync::Arc::clone(&watcher_calls);
        let root = repo.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let outcome = wait_oneshot_peer(
                &root,
                || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(None)
                },
                || {
                    flag.store(true, Ordering::SeqCst);
                },
            );
            tx.send(matches!(outcome, Ok(OneshotPeer::Lock(_))))
                .unwrap();
        });
        let start = Instant::now();
        while !waited.load(Ordering::SeqCst) {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "oneshot must wait for a peer kiss test lock"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(rx.try_recv().is_err(), "waiter must stay blocked");
        assert_eq!(
            watcher_calls.load(Ordering::SeqCst),
            0,
            "no watcher session, so oneshot must not probe the watcher"
        );
        drop(peer);
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "waiter must acquire the lock after the peer exits"
        );
    }

    #[test]
    fn wait_oneshot_peer_returns_watcher_exit_when_session_live() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let session = super::super::control::SessionFile {
            pid: std::process::id(),
            socket: repo.join("nudge.sock").display().to_string(),
        };
        super::super::control::write_session_file(
            &super::super::control::session_file_path(repo),
            &session,
        )
        .unwrap();
        let outcome = wait_oneshot_peer(repo, || Ok(Some(7)), || {});
        assert!(matches!(outcome, Ok(OneshotPeer::WatcherExit(7))));
    }

    #[test]
    fn wait_oneshot_peer_surfaces_lock_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("not-a-dir");
        std::fs::write(&repo, "file").unwrap();
        let outcome = wait_oneshot_peer(&repo, || Ok(None), || {});
        assert!(outcome.is_err(), "lock under a file path must error");
    }

    #[test]
    fn wait_oneshot_peer_continues_when_live_watcher_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let session = super::super::control::SessionFile {
            pid: std::process::id(),
            socket: repo.join("nudge.sock").display().to_string(),
        };
        super::super::control::write_session_file(
            &super::super::control::session_file_path(repo),
            &session,
        )
        .unwrap();
        let calls = AtomicUsize::new(0);
        let outcome = wait_oneshot_peer(
            repo,
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            },
            || {},
        );
        assert!(matches!(outcome, Ok(OneshotPeer::Lock(_))));
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn wait_oneshot_peer_propagates_watcher_probe_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let session = super::super::control::SessionFile {
            pid: std::process::id(),
            socket: repo.join("nudge.sock").display().to_string(),
        };
        super::super::control::write_session_file(
            &super::super::control::session_file_path(repo),
            &session,
        )
        .unwrap();
        let outcome = wait_oneshot_peer(repo, || Err("probe failed".into()), || {});
        assert!(outcome.is_err());
    }
}
