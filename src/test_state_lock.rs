use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;

thread_local! {
    static HELD: RefCell<HashMap<PathBuf, (File, usize)>> = RefCell::new(HashMap::new());
}

pub struct TestStateLock {
    path: PathBuf,
    _same_thread: PhantomData<*const ()>,
}

pub fn lock_test_state_dir(state_dir: &Path) -> io::Result<TestStateLock> {
    acquire(state_dir, None)?.ok_or_else(|| io::Error::other("writer lock not acquired"))
}

pub fn lock_test_state_dir_within(
    state_dir: &Path,
    timeout: Duration,
) -> io::Result<Option<TestStateLock>> {
    acquire(state_dir, Some(timeout))
}

fn acquire(state_dir: &Path, timeout: Option<Duration>) -> io::Result<Option<TestStateLock>> {
    fs::create_dir_all(state_dir)?;
    let path = state_dir.canonicalize()?.join("writer.lock");
    let nested = HELD.with(|held| {
        held.borrow_mut()
            .get_mut(&path)
            .map(|(_, depth)| *depth += 1)
            .is_some()
    });
    if !nested {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match timeout {
            None => file.lock_exclusive()?,
            Some(timeout) => {
                if !wait_for_lock(&file, timeout)? {
                    return Ok(None);
                }
            }
        }
        HELD.with(|held| held.borrow_mut().insert(path.clone(), (file, 1)));
        discard_retired_state(state_dir);
    }
    Ok(Some(TestStateLock {
        path,
        _same_thread: PhantomData,
    }))
}

fn wait_for_lock(file: &File, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(true),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(err),
        }
    }
}

impl Drop for TestStateLock {
    fn drop(&mut self) {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            if let Some((_, depth)) = held.get_mut(&self.path) {
                *depth -= 1;
                if *depth == 0 {
                    held.remove(&self.path);
                }
            }
        });
    }
}

const RETIRED_PATHS: &[&str] = &[
    "check_runtime_coverage_locks",
    "rslip_cache",
    "cov_records_cache.json",
    "test_last_status.json",
    "target-reports",
    "rust_llvm_cov_cache",
    "profraw",
    "rust_tool_versions.json",
    "watch",
];
const RETIRED_PREFIXES: &[&str] = &["cargo_roots_v1_", "cargo_roots_v2_"];

pub fn discard_retired_state_if_idle(state_dir: &Path) {
    let Ok(path) = state_dir.join("writer.lock").canonicalize() else {
        return;
    };
    if HELD.with(|held| held.borrow().contains_key(&path)) {
        return;
    }
    let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
        return;
    };
    if file.try_lock_exclusive().is_ok() {
        discard_retired_state(state_dir);
    }
}

fn discard_retired_state(state_dir: &Path) {
    for rel in RETIRED_PATHS {
        remove_path(&state_dir.join(rel));
    }
    let Ok(entries) = fs::read_dir(state_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if RETIRED_PREFIXES
            .iter()
            .any(|prefix| name.to_string_lossy().starts_with(prefix))
        {
            remove_path(&entry.path());
        }
    }
}

fn remove_path(path: &Path) {
    let _ = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(_) => Ok(()),
    };
}

pub fn owning_test_state_dir(path: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(path)?;
    let path = path.canonicalize()?;
    Ok(path
        .ancestors()
        .find(|ancestor| ancestor.ends_with(".kiss/test"))
        .unwrap_or(&path)
        .to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn first_acquisition_discards_retired_state_and_keeps_current_state() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path();
        let cache = state.join("rust_llvm_cov_cache");
        fs::create_dir_all(cache.join("generations").join("g")).unwrap();
        fs::create_dir_all(cache.join("entries")).unwrap();
        fs::create_dir_all(state.join("target-reports").join("r")).unwrap();
        fs::create_dir_all(state.join("records").join("rust")).unwrap();
        fs::create_dir_all(state.join("rslip_cache").join("hosts")).unwrap();
        fs::write(cache.join("current_generation.json"), b"{}").unwrap();
        fs::write(state.join("test_last_status.json"), b"{}").unwrap();
        fs::write(state.join("cargo_roots_v2_00.bin"), b"").unwrap();
        fs::write(state.join("cargo_roots_v3_00.bin"), b"").unwrap();
        drop(lock_test_state_dir(state).unwrap());
        fs::write(state.join("test_last_status.json"), b"{}").unwrap();
        discard_retired_state_if_idle(state);
        fs::write(state.join("cargo_roots_v2_00.bin"), b"").unwrap();
        {
            let _held = lock_test_state_dir(state).unwrap();
            fs::write(state.join("cargo_roots_v2_00.bin"), b"").unwrap();
            discard_retired_state_if_idle(state);
            assert!(state.join("cargo_roots_v2_00.bin").exists());
        }
        drop(lock_test_state_dir(state).unwrap());
        for gone in [
            "rust_llvm_cov_cache",
            "rslip_cache",
            "target-reports",
            "test_last_status.json",
            "cargo_roots_v2_00.bin",
        ] {
            assert!(!state.join(gone).exists(), "{gone} must be discarded");
        }
        for kept in ["records/rust", "cargo_roots_v3_00.bin"] {
            assert!(state.join(kept).exists(), "{kept} must stay");
        }
    }

    #[test]
    fn nested_acquisitions_on_one_thread_do_not_deadlock() {
        let tmp = tempfile::tempdir().unwrap();
        let outer: TestStateLock = lock_test_state_dir(tmp.path()).unwrap();
        let inner: TestStateLock = lock_test_state_dir(tmp.path()).unwrap();
        drop(inner);
        drop(outer);
        assert!(tmp.path().join("writer.lock").is_file());
    }

    #[test]
    fn other_threads_wait_until_the_outermost_guard_drops() {
        let tmp = tempfile::tempdir().unwrap();
        let outer = lock_test_state_dir(tmp.path()).unwrap();
        let inner = lock_test_state_dir(tmp.path()).unwrap();
        let (tx, rx) = mpsc::channel();
        let dir = tmp.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            let _guard = lock_test_state_dir(&dir).unwrap();
            tx.send(()).unwrap();
        });
        drop(inner);
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(outer);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn owning_state_dir_is_the_nearest_kiss_test_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join(".kiss").join("test");
        let cache = state.join("records").join("python").join("h");
        assert_eq!(
            owning_test_state_dir(&cache).unwrap(),
            state.canonicalize().unwrap()
        );
        let loose = tmp.path().join("loose");
        assert_eq!(
            owning_test_state_dir(&loose).unwrap(),
            loose.canonicalize().unwrap()
        );
    }

    #[test]
    fn timeout_returns_the_lock_when_the_directory_is_free() {
        let tmp = tempfile::tempdir().unwrap();
        let guard = lock_test_state_dir_within(tmp.path(), Duration::from_secs(1)).unwrap();
        assert!(guard.is_some());
    }

    #[test]
    fn a_file_in_place_of_the_state_dir_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("not-a-directory");
        fs::write(&file, b"file").unwrap();
        assert!(lock_test_state_dir(&file).is_err());
        assert!(owning_test_state_dir(&file).is_err());
    }
}
