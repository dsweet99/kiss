use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub(crate) fn inject_selector_lock_failure(cache_root: &Path, fingerprint: &str) {
    inject_lock_failure(
        cache_root
            .join("locks")
            .join("selectors")
            .join(format!("{fingerprint}.lock")),
    );
}

pub(crate) fn inject_legacy_cleanup_lock_failure(cache_root: &Path) {
    inject_lock_failure(
        cache_root
            .join("locks")
            .join("workers")
            .join("legacy-cleanup.lock"),
    );
}

pub(crate) fn inject_worker_lock_failure(cache_root: &Path, worker_slot: usize) {
    inject_lock_failure(
        cache_root
            .join("locks")
            .join("workers")
            .join(format!("slot-{worker_slot}.lock")),
    );
}

pub(crate) fn fail_if_injected(path: &Path) -> io::Result<()> {
    let mut failures = injected_lock_failures().lock().unwrap();
    let Some(index) = failures.iter().position(|candidate| candidate == path) else {
        return Ok(());
    };
    failures.remove(index);
    Err(io::Error::other(format!(
        "injected lock failure for {}",
        path.display()
    )))
}

fn injected_lock_failures() -> &'static Mutex<Vec<PathBuf>> {
    static FAILURES: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    FAILURES.get_or_init(|| Mutex::new(Vec::new()))
}

fn inject_lock_failure(path: PathBuf) {
    injected_lock_failures().lock().unwrap().push(path);
}
