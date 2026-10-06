use std::io;
use std::path::Path;

use crate::test_state_lock::{TestStateLock, lock_test_state_dir, owning_test_state_dir};

pub fn lock_rslip_state(cache_root: &Path) -> io::Result<TestStateLock> {
    lock_test_state_dir(&owning_test_state_dir(cache_root)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn repository_cache_roots_share_the_state_dir_writer_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join(".kiss").join("test");
        let cache = state.join("rslip_cache").join("hosts").join("h");
        let _guard = lock_rslip_state(&cache).unwrap();
        assert!(state.join("writer.lock").is_file());
        assert!(!cache.join("locks").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_roots_contend_on_one_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");
        let link = tmp.path().join("cache-link");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let guard = lock_rslip_state(&root).unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _guard = lock_rslip_state(&link).unwrap();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(guard);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn invalid_cache_root_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let file_root = tmp.path().join("not-a-directory");
        fs::write(&file_root, b"file").unwrap();
        assert!(lock_rslip_state(&file_root).is_err());
    }
}
