use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt;

pub struct LocalRslipLockGuard {
    pub(crate) _file: File,
}

pub fn lock_rslip_cache_entry(
    cache_root: &Path,
    fingerprint: &str,
) -> io::Result<LocalRslipLockGuard> {
    lock_rslip_path(&rslip_cache_entry_lock_path(cache_root, fingerprint)?)
}

pub fn lock_rslip_derived_state(cache_root: &Path) -> io::Result<LocalRslipLockGuard> {
    lock_rslip_path(&rslip_derived_state_lock_path(cache_root)?)
}

fn lock_rslip_path(path: &Path) -> io::Result<LocalRslipLockGuard> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("rslip lock path has no parent"))?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(LocalRslipLockGuard { _file: file })
}

pub(crate) fn rslip_cache_entry_lock_path(
    cache_root: &Path,
    fingerprint: &str,
) -> io::Result<PathBuf> {
    Ok(rslip_lock_root(cache_root)?
        .join("entries")
        .join(format!("{fingerprint}.lock")))
}

pub(crate) fn rslip_derived_state_lock_path(cache_root: &Path) -> io::Result<PathBuf> {
    Ok(rslip_lock_root(cache_root)?.join("derived.lock"))
}

fn rslip_lock_root(cache_root: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(cache_root)?;
    let canonical = cache_root.canonicalize()?;
    Ok(Path::new("/tmp")
        .join("kiss-rslip-locks")
        .join(hex_encode_path(canonical.as_os_str().as_encoded_bytes())))
}

fn hex_encode_path(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn lock_paths_are_stable_for_same_cache_root_and_distinct_between_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");

        let first_path = rslip_cache_entry_lock_path(&first, "abc").unwrap();
        let first_again = rslip_cache_entry_lock_path(&first, "abc").unwrap();
        let second_path = rslip_cache_entry_lock_path(&second, "abc").unwrap();

        assert_eq!(first_path, first_again);
        assert_ne!(first_path, second_path);
        assert!(first_path.starts_with("/tmp/kiss-rslip-locks"));
        assert!(first_path.ends_with("entries/abc.lock"));
        assert!(
            rslip_derived_state_lock_path(&first)
                .unwrap()
                .ends_with("derived.lock")
        );
        assert!(std::any::type_name::<LocalRslipLockGuard>().contains("LocalRslipLockGuard"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cache_roots_map_to_same_lock_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");
        let link = tmp.path().join("cache-link");
        fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(&root, &link).unwrap();

        let direct = rslip_cache_entry_lock_path(&root, "abc").unwrap();
        let symlinked = rslip_cache_entry_lock_path(&link, "abc").unwrap();

        assert_eq!(direct, symlinked);
        assert!(direct.starts_with("/tmp/kiss-rslip-locks"));
    }

    #[test]
    fn dropping_guard_releases_same_path_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");

        let guard = lock_rslip_cache_entry(&root, "abc").unwrap();
        drop(guard);
        let _second = lock_rslip_cache_entry(&root, "abc").unwrap();
    }

    #[test]
    fn derived_lock_uses_same_root_and_invalid_cache_root_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");

        let _guard = lock_rslip_derived_state(&root).unwrap();
        let path = rslip_derived_state_lock_path(&root).unwrap();
        assert!(path.is_file());

        let file_root = tmp.path().join("not-a-directory");
        fs::write(&file_root, b"file").unwrap();
        assert!(lock_rslip_cache_entry(&file_root, "abc").is_err());
    }

    #[test]
    fn same_path_locks_serialize_while_distinct_paths_do_not() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache");
        let guard = lock_rslip_cache_entry(&root, "abc").unwrap();
        let (tx, rx) = mpsc::channel();
        let root_for_thread = root.clone();
        let handle = thread::spawn(move || {
            let _guard = lock_rslip_cache_entry(&root_for_thread, "abc").unwrap();
            tx.send(()).unwrap();
        });

        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        let _different = lock_rslip_cache_entry(&root, "def").unwrap();
        drop(guard);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        handle.join().unwrap();
    }
}
