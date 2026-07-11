use std::path::{Path, PathBuf};

use crate::file_lock::FileLockGuard;

pub(crate) fn batch_lock_path(cache_root: &Path) -> PathBuf {
    cache_root.join("locks").join("batch.lock")
}

pub(crate) fn lock_batch(cache_root: &Path) -> std::io::Result<FileLockGuard> {
    FileLockGuard::lock(&batch_lock_path(cache_root))
}

#[allow(dead_code)]
pub(crate) fn try_lock_batch(cache_root: &Path) -> std::io::Result<Option<FileLockGuard>> {
    FileLockGuard::try_lock(&batch_lock_path(cache_root))
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
    }
}
