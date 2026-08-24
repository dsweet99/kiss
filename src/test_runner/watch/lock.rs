use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;

pub(crate) struct WatchLockGuard {
    _file: File,
}

impl WatchLockGuard {
    #[cfg(test)]
    pub(crate) fn lock(path: &Path) -> io::Result<Self> {
        let file = open_watch_lock(path)?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }

    pub(crate) fn try_lock(path: &Path) -> io::Result<Option<Self>> {
        let file = open_watch_lock(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub(crate) fn try_lock_shared(path: &Path) -> io::Result<Option<Self>> {
        let file = open_watch_lock(path)?;
        match FileExt::try_lock_shared(&file) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn open_watch_lock(path: &Path) -> io::Result<File> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::other("watch lock path has no parent directory"));
    };
    std::fs::create_dir_all(parent)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}

pub(crate) fn watch_lock_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("watch").join("watch.lock")
}

pub(crate) fn watch_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("watch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_lock_success_vs_would_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("watch.lock");
        let first = WatchLockGuard::try_lock(&path)
            .unwrap()
            .expect("first lock");
        assert!(WatchLockGuard::try_lock(&path).unwrap().is_none());
        drop(first);
        assert!(WatchLockGuard::try_lock(&path).unwrap().is_some());
    }

    #[test]
    fn try_lock_release_allows_second_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let path = watch_lock_path(tmp.path());
        let first = WatchLockGuard::try_lock(&path).unwrap().expect("first");
        drop(first);
        let second = WatchLockGuard::try_lock(&path).unwrap().expect("second");
        drop(second);
    }

    #[test]
    fn shared_probes_do_not_exclude_each_other() {
        let tmp = tempfile::tempdir().unwrap();
        let path = watch_lock_path(tmp.path());
        let first = WatchLockGuard::try_lock_shared(&path)
            .unwrap()
            .expect("first shared");
        let second = WatchLockGuard::try_lock_shared(&path)
            .unwrap()
            .expect("second shared");
        assert!(WatchLockGuard::try_lock(&path).unwrap().is_none());
        drop(first);
        drop(second);
        assert!(WatchLockGuard::try_lock(&path).unwrap().is_some());
    }

    #[test]
    fn exclusive_holder_blocks_shared_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let path = watch_lock_path(tmp.path());
        let exclusive = WatchLockGuard::try_lock(&path).unwrap().expect("exclusive");
        assert!(WatchLockGuard::try_lock_shared(&path).unwrap().is_none());
        drop(exclusive);
        assert!(WatchLockGuard::try_lock_shared(&path).unwrap().is_some());
    }
}
