use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::file_lock::FileLockGuard;

pub(crate) fn batch_lock_path(cache_root: &Path) -> PathBuf {
    cache_root.join("locks").join("batch.lock")
}

#[cfg(test)]
pub(crate) fn lock_batch(cache_root: &Path) -> std::io::Result<FileLockGuard> {
    FileLockGuard::lock(&batch_lock_path(cache_root))
}

pub(crate) fn try_lock_batch(cache_root: &Path) -> std::io::Result<Option<FileLockGuard>> {
    FileLockGuard::try_lock(&batch_lock_path(cache_root))
}

pub(crate) fn wait_for_batch_lock(cache_root: &Path) -> std::io::Result<FileLockGuard> {
    wait_for_batch_lock_for(cache_root, std::time::Duration::from_secs(30))
}

fn wait_for_batch_lock_for(
    cache_root: &Path,
    timeout: std::time::Duration,
) -> std::io::Result<FileLockGuard> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(guard) = try_lock_batch(cache_root)? {
            return Ok(guard);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "timed out after {:.1}s waiting for rust batch lock",
                    timeout.as_secs_f64()
                ),
            ));
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(now)
                .min(std::time::Duration::from_millis(250)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_lock_serializes_two_processes() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_root = tmp.path();
        let _first = lock_batch(cache_root).unwrap();
        assert!(try_lock_batch(cache_root).unwrap().is_none());
        let err = match wait_for_batch_lock_for(cache_root, std::time::Duration::from_millis(1)) {
            Ok(_) => panic!("held lock must time out"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }
}
