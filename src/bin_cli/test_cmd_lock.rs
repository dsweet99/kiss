use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

const ONESHOT_LOCK_POLL: Duration = Duration::from_millis(10);

pub(super) struct OneshotLockGuard {
    _file: File,
}

impl OneshotLockGuard {
    fn try_lock(path: &Path) -> io::Result<Option<Self>> {
        let Some(parent) = path.parent() else {
            return Err(io::Error::other(
                "kiss test lock path has no parent directory",
            ));
        };
        std::fs::create_dir_all(parent)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn oneshot_lock_path(repo_root: &Path) -> PathBuf {
    crate::test_runner::test_state_dir(repo_root).join("kiss_test.lock")
}

fn wait_oneshot_lock(
    lock_path: &Path,
    mut on_wait: impl FnMut(),
) -> Result<OneshotLockGuard, String> {
    loop {
        match OneshotLockGuard::try_lock(lock_path) {
            Ok(Some(guard)) => return Ok(guard),
            Ok(None) => {
                on_wait();
                thread::sleep(ONESHOT_LOCK_POLL);
            }
            Err(e) => return Err(format!("cannot lock {}: {e}", lock_path.display())),
        }
    }
}

fn note_oneshot_wait(next: &mut Option<Instant>) {
    let now = Instant::now();
    if next.is_none_or(|at| now >= at) {
        crate::test_runner::emit_test_progress("kiss test: waiting for kiss test");
        *next = Some(now + Duration::from_secs(3));
    }
}

pub(super) fn take_oneshot_lock() -> Result<OneshotLockGuard, i32> {
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let repo_root = crate::test_git::require_git_repo_root(&cwd).map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })?;
    let mut next_wait = None;
    wait_oneshot_lock(&oneshot_lock_path(&repo_root), || {
        note_oneshot_wait(&mut next_wait);
    })
    .map_err(|e| {
        eprintln!("error: kiss test: {e}");
        1
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};

    #[test]
    fn note_oneshot_wait_prints_then_waits() {
        let mut next = None;
        note_oneshot_wait(&mut next);
        assert!(next.is_some());
        let stamped = next;
        note_oneshot_wait(&mut next);
        assert_eq!(next, stamped);
    }

    #[test]
    fn two_worktrees_do_not_share_a_lock() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_ne!(oneshot_lock_path(a.path()), oneshot_lock_path(b.path()));
    }

    #[test]
    fn try_lock_excludes_until_release() {
        let tmp = tempfile::tempdir().unwrap();
        let path = oneshot_lock_path(tmp.path());
        let first = OneshotLockGuard::try_lock(&path).unwrap().expect("first");
        assert!(OneshotLockGuard::try_lock(&path).unwrap().is_none());
        drop(first);
        assert!(OneshotLockGuard::try_lock(&path).unwrap().is_some());
    }

    #[test]
    fn wait_oneshot_lock_blocks_until_peer_releases() {
        let tmp = tempfile::tempdir().unwrap();
        let path = oneshot_lock_path(tmp.path());
        let peer = OneshotLockGuard::try_lock(&path).unwrap().expect("peer");
        let waited = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&waited);
        let thread_path = path.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let outcome = wait_oneshot_lock(&thread_path, || flag.store(true, Ordering::SeqCst));
            tx.send(outcome.is_ok()).unwrap();
        });
        let start = Instant::now();
        while !waited.load(Ordering::SeqCst) {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "waiter must poll while the peer holds the lock"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(rx.try_recv().is_err(), "waiter must stay blocked");
        drop(peer);
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
    }

    #[test]
    fn wait_oneshot_lock_surfaces_lock_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, "file").unwrap();
        assert!(wait_oneshot_lock(&blocker.join("kiss_test.lock"), || {}).is_err());
    }
}
