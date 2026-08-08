//! Tolerate `NotFound` races when concurrent publishers create/delete paths.

use std::fs::{self, DirEntry, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

/// `Ok(None)` when the directory is missing; otherwise the open `ReadDir`.
pub(crate) fn read_dir_ok_missing(path: &Path) -> io::Result<Option<ReadDir>> {
    match fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Map a `read_dir` item, skipping entries that vanished mid-iteration.
pub(crate) fn dir_entry_ok_missing(entry: io::Result<DirEntry>) -> io::Result<Option<DirEntry>> {
    match entry {
        Ok(entry) => Ok(Some(entry)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Read file bytes, treating a vanished path as `Ok(None)`.
pub(crate) fn read_ok_missing(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Remove a file, treating a vanished path as success.
pub(crate) fn remove_file_ok_missing(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Remove a directory tree, treating a vanished path as success.
pub(crate) fn remove_dir_all_ok_missing(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// `DirEntry::file_type`, treating a vanished path as `Ok(None)`.
pub(crate) fn file_type_ok_missing(entry: &DirEntry) -> io::Result<Option<std::fs::FileType>> {
    match entry.file_type() {
        Ok(file_type) => Ok(Some(file_type)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Path of a `read_dir` item, or `None` if it vanished.
pub(crate) fn dir_entry_path_ok_missing(
    entry: io::Result<DirEntry>,
) -> io::Result<Option<PathBuf>> {
    Ok(dir_entry_ok_missing(entry)?.map(|entry| entry.path()))
}
