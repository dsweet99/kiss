use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::file_lock::FileLockGuard;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RustLegacyCleanupAttempt {
    pub(crate) deferred: bool,
}

pub fn rust_cov_cache_tmp_parent(cache_root: &Path) -> PathBuf {
    std::env::temp_dir()
        .join("kiss-rust-llvm-cov")
        .join(format!("cache-{}", cache_root_digest(cache_root)))
}

pub(crate) fn rust_cov_worker_slot_root(cache_root: &Path, worker_slot: usize) -> PathBuf {
    cache_root
        .join("workers")
        .join(format!("slot-{worker_slot}"))
}

pub(crate) fn rust_cov_worker_tmp_root(cache_root: &Path, worker_slot: usize) -> PathBuf {
    rust_cov_cache_tmp_parent(cache_root).join(format!("slot-{worker_slot}"))
}

pub(crate) fn cache_root_digest(cache_root: &Path) -> String {
    let canonical = fs::canonicalize(cache_root).unwrap_or_else(|_| cache_root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(os_str_bytes(canonical.as_os_str()));
    hex_lower(&hasher.finalize())
}

#[cfg(unix)]
pub(crate) fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub(crate) fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

pub(crate) fn cleanup_legacy_worker_dirs(cache_root: &Path) -> io::Result<()> {
    let workers_root = cache_root.join("workers");
    let Ok(entries) = fs::read_dir(&workers_root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_legacy_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("slot-"));
        if is_legacy_dir {
            match fs::remove_dir_all(path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
    }
    Ok(())
}

pub(crate) fn cleanup_legacy_worker_data_nonblocking(
    cache_root: &Path,
) -> io::Result<RustLegacyCleanupAttempt> {
    let Some(_legacy_guard) = try_lock_legacy_cleanup(cache_root)? else {
        return Ok(RustLegacyCleanupAttempt { deferred: true });
    };
    let slots = legacy_worker_slots(cache_root)?;
    let mut slot_guards = Vec::with_capacity(slots.len());
    for slot in &slots {
        let Some(guard) = try_lock_worker(cache_root, *slot)? else {
            return Ok(RustLegacyCleanupAttempt { deferred: true });
        };
        slot_guards.push(guard);
    }
    for slot in slots {
        remove_worker_slot_roots(cache_root, slot)?;
    }
    cleanup_legacy_worker_dirs(cache_root)?;
    drop(slot_guards);
    Ok(RustLegacyCleanupAttempt { deferred: false })
}

fn try_lock_legacy_cleanup(cache_root: &Path) -> io::Result<Option<FileLockGuard>> {
    let path = cache_root
        .join("locks")
        .join("workers")
        .join("legacy-cleanup.lock");
    FileLockGuard::try_lock(&path)
}

fn try_lock_worker(cache_root: &Path, worker_slot: usize) -> io::Result<Option<FileLockGuard>> {
    let path = worker_lock_path(cache_root, worker_slot);
    FileLockGuard::try_lock(&path)
}

fn worker_lock_path(cache_root: &Path, worker_slot: usize) -> PathBuf {
    cache_root
        .join("locks")
        .join("workers")
        .join(format!("slot-{worker_slot}.lock"))
}

fn legacy_worker_slots(cache_root: &Path) -> io::Result<Vec<usize>> {
    let workers_root = cache_root.join("workers");
    let Ok(entries) = fs::read_dir(&workers_root) else {
        return Ok(Vec::new());
    };
    let mut slots = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(slot) = parse_worker_slot_name(&entry.file_name()) {
            slots.push(slot);
        }
    }
    slots.sort_unstable();
    Ok(slots)
}

fn parse_worker_slot_name(name: &OsStr) -> Option<usize> {
    name.to_str()?.strip_prefix("slot-")?.parse().ok()
}

#[cfg(test)]
pub(crate) fn lock_worker_for_test(cache_root: &Path, worker_slot: usize) -> io::Result<FileLockGuard> {
    FileLockGuard::lock(&worker_lock_path(cache_root, worker_slot))
}

fn remove_worker_slot_roots(cache_root: &Path, worker_slot: usize) -> io::Result<()> {
    remove_path_if_exists(&rust_cov_worker_slot_root(cache_root, worker_slot))?;
    remove_path_if_exists(&rust_cov_worker_tmp_root(cache_root, worker_slot))?;
    cleanup_empty_tmp_parent(cache_root)
}

fn cleanup_empty_tmp_parent(cache_root: &Path) -> io::Result<()> {
    match fs::remove_dir(rust_cov_cache_tmp_parent(cache_root)) {
        Ok(()) => Ok(()),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn remove_path_if_exists(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
