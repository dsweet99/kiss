
use std::fs::{self, DirEntry, ReadDir};
use std::io;
use std::path::{Path, PathBuf};

pub(crate) fn read_dir_ok_missing(path: &Path) -> io::Result<Option<ReadDir>> {
    match fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(crate) fn dir_entry_ok_missing(entry: io::Result<DirEntry>) -> io::Result<Option<DirEntry>> {
    match entry {
        Ok(entry) => Ok(Some(entry)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(crate) fn read_ok_missing(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(crate) fn remove_file_ok_missing(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn remove_dir_all_ok_missing(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn file_type_ok_missing(entry: &DirEntry) -> io::Result<Option<std::fs::FileType>> {
    match entry.file_type() {
        Ok(file_type) => Ok(Some(file_type)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub(crate) fn dir_entry_path_ok_missing(
    entry: io::Result<DirEntry>,
) -> io::Result<Option<PathBuf>> {
    Ok(dir_entry_ok_missing(entry)?.map(|entry| entry.path()))
}
