use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::RustLlvmCovError;
use crate::file_lock::FileLockGuard;

#[cfg(test)]
pub(crate) mod lock_failure_injection;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RustWorkerCleanupReport {
    pub removed_slots: Vec<usize>,
    pub skipped_slots: Vec<usize>,
}

pub fn cleanup_surplus_rust_cov_worker_slots(
    cache_root: &Path,
    jobs: usize,
) -> Result<RustWorkerCleanupReport, RustLlvmCovError> {
    assert!(jobs > 0, "jobs must be greater than zero");
    fs::create_dir_all(cache_root)?;
    let workers_root = cache_root.join("workers");
    let Ok(entries) = fs::read_dir(&workers_root) else {
        return Ok(RustWorkerCleanupReport::default());
    };
    let mut report = RustWorkerCleanupReport::default();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(slot) = parse_worker_slot_name(&entry.file_name()) else {
            continue;
        };
        if slot < jobs {
            continue;
        }
        match try_lock_worker(cache_root, slot)? {
            Some(_guard) => {
                remove_worker_slot_roots(cache_root, slot)?;
                report.removed_slots.push(slot);
            }
            None => report.skipped_slots.push(slot),
        }
    }
    report.removed_slots.sort_unstable();
    report.skipped_slots.sort_unstable();
    Ok(report)
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

pub(crate) fn prepare_worker_slot(cache_root: &Path, worker_slot: usize) -> io::Result<PathBuf> {
    let worker_root = rust_cov_worker_slot_root(cache_root, worker_slot);
    fs::create_dir_all(&worker_root)?;
    cleanup_worker_slot_transients(cache_root, worker_slot)?;
    Ok(worker_root)
}

pub(crate) fn cleanup_worker_slot_transients(
    cache_root: &Path,
    worker_slot: usize,
) -> io::Result<()> {
    let worker_root = rust_cov_worker_slot_root(cache_root, worker_slot);
    for name in ["profile", "tmp"] {
        let path = worker_root.join(name);
        remove_path_if_exists(&path)?;
    }
    remove_path_if_exists(&rust_cov_worker_tmp_root(cache_root, worker_slot))?;
    cleanup_empty_tmp_parent(cache_root)?;
    Ok(())
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

fn remove_worker_slot_roots(cache_root: &Path, worker_slot: usize) -> io::Result<()> {
    remove_path_if_exists(&rust_cov_worker_slot_root(cache_root, worker_slot))?;
    remove_path_if_exists(&rust_cov_worker_tmp_root(cache_root, worker_slot))?;
    cleanup_empty_tmp_parent(cache_root)
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

pub(crate) fn lock_selector(cache_root: &Path, fingerprint: &str) -> io::Result<FileLockGuard> {
    let path = cache_root
        .join("locks")
        .join("selectors")
        .join(format!("{fingerprint}.lock"));
    #[cfg(test)]
    lock_failure_injection::fail_if_injected(&path)?;
    FileLockGuard::lock(&path)
}

pub(crate) fn lock_legacy_cleanup(cache_root: &Path) -> io::Result<FileLockGuard> {
    let path = cache_root
        .join("locks")
        .join("workers")
        .join("legacy-cleanup.lock");
    #[cfg(test)]
    lock_failure_injection::fail_if_injected(&path)?;
    FileLockGuard::lock(&path)
}

pub(crate) fn lock_worker(cache_root: &Path, worker_slot: usize) -> io::Result<FileLockGuard> {
    let path = worker_lock_path(cache_root, worker_slot);
    #[cfg(test)]
    lock_failure_injection::fail_if_injected(&path)?;
    FileLockGuard::lock(&path)
}

fn try_lock_worker(cache_root: &Path, worker_slot: usize) -> io::Result<Option<FileLockGuard>> {
    let path = worker_lock_path(cache_root, worker_slot);
    #[cfg(test)]
    lock_failure_injection::fail_if_injected(&path)?;
    FileLockGuard::try_lock(&path)
}

fn worker_lock_path(cache_root: &Path, worker_slot: usize) -> PathBuf {
    cache_root
        .join("locks")
        .join("workers")
        .join(format!("slot-{worker_slot}.lock"))
}

fn parse_worker_slot_name(name: &OsStr) -> Option<usize> {
    name.to_str()?.strip_prefix("slot-")?.parse().ok()
}

#[cfg(test)]
pub(crate) fn wait_at_unlocked_miss_hook() -> io::Result<()> {
    let Ok(ready_path) = std::env::var("KISS_RUST_COV_UNLOCKED_MISS_READY") else {
        return Ok(());
    };
    let go_path = std::env::var("KISS_RUST_COV_UNLOCKED_MISS_GO")
        .map_err(|_| io::Error::other("missing unlocked miss go path"))?;
    fs::write(&ready_path, b"ready")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !Path::new(&go_path).exists() {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting at unlocked miss hook",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}
