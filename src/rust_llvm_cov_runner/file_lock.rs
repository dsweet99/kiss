use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use fs2::FileExt;

pub(crate) struct FileLockGuard {
    _file: File,
}

impl FileLockGuard {
    pub(crate) fn lock(path: &Path) -> io::Result<Self> {
        let file = open_lock_file(path)?;
        file.lock_exclusive()?;
        Ok(Self { _file: file })
    }

    pub(crate) fn try_lock(path: &Path) -> io::Result<Option<Self>> {
        let file = open_lock_file(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("lock path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}
