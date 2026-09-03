use std::path::PathBuf;

pub(crate) struct JobGuard;

pub(crate) fn install_job(
    _repo_root: PathBuf,
    _extra: Vec<String>,
    _jobs: usize,
    _dry_run: bool,
) -> JobGuard {
    JobGuard
}

pub(crate) fn covering_population_list_build_done() -> bool {
    false
}

pub(crate) fn overlap_with_discover<T>(
    discover: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    discover()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn covering_discovery_does_not_spawn_detached_list_build() {
        let ran = AtomicBool::new(false);
        let _guard = install_job(PathBuf::from("."), Vec::new(), 1, false);
        let value = overlap_with_discover(|| {
            ran.store(true, Ordering::SeqCst);
            Ok(4)
        })
        .unwrap();
        assert_eq!(value, 4);
        assert!(ran.load(Ordering::SeqCst));
        assert!(!covering_population_list_build_done());
    }
}
