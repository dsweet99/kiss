use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

thread_local! {
    static JOB: RefCell<Option<RustListBuildJob>> = const { RefCell::new(None) };
    static HANDLE: RefCell<Option<JoinHandle<Result<(), String>>>> = const { RefCell::new(None) };
    static COVERING_OVERLAPPED: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
thread_local! {
    static TEST_HOOK: RefCell<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> =
        const { RefCell::new(None) };
}

#[derive(Clone)]
struct RustListBuildJob {
    repo_root: PathBuf,
    extra: Vec<String>,
    jobs: usize,
    dry_run: bool,
}

pub(crate) struct JobGuard;

impl Drop for JobGuard {
    fn drop(&mut self) {
        let _ = join_started();
        JOB.with(|job| *job.borrow_mut() = None);
        COVERING_OVERLAPPED.with(|flag| flag.set(false));
    }
}

pub(crate) fn install_job(
    repo_root: PathBuf,
    extra: Vec<String>,
    jobs: usize,
    dry_run: bool,
) -> JobGuard {
    COVERING_OVERLAPPED.with(|flag| flag.set(false));
    HANDLE.with(|handle| *handle.borrow_mut() = None);
    JOB.with(|job| {
        *job.borrow_mut() = Some(RustListBuildJob {
            repo_root,
            extra,
            jobs,
            dry_run,
        });
    });
    JobGuard
}

pub(crate) fn covering_population_list_build_done() -> bool {
    COVERING_OVERLAPPED.with(Cell::get)
}

pub(crate) fn should_overlap(repo_root: &Path) -> bool {
    repo_root.join("Cargo.toml").is_file()
        && std::env::var_os("NEXTEST").is_none()
        && std::env::var_os("CARGO_LLVM_COV").is_none()
}

pub(crate) fn overlap_with_discover<T>(
    discover: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let started = begin()?;
    let discovered = discover();
    let join_err = join_started();
    let value = discovered?;
    join_err?;
    if started {
        COVERING_OVERLAPPED.with(|flag| flag.set(true));
    }
    Ok(value)
}

fn spawn_list_build(work: impl FnOnce() -> Result<(), String> + Send + 'static) {
    HANDLE.with(|handle| {
        *handle.borrow_mut() = Some(std::thread::spawn(work));
    });
}

fn begin() -> Result<bool, String> {
    if HANDLE.with(|handle| handle.borrow().is_some()) {
        return Ok(true);
    }
    let Some(job) = JOB.with(|slot| slot.borrow().clone()) else {
        return Ok(false);
    };
    #[cfg(test)]
    if let Some(hook) = TEST_HOOK.with(|slot| slot.borrow().clone()) {
        spawn_list_build(move || {
            hook();
            Ok(())
        });
        return Ok(true);
    }
    if job.dry_run || !should_overlap(&job.repo_root) {
        return Ok(false);
    }
    let cache_root = job.repo_root.join(".kiss").join("rust_llvm_cov_cache");
    kiss::rust_llvm_cov_runner::lock_and_hold_batch(&cache_root)
        .map_err(|err| format!("error: kiss test: rust batch lock: {err}"))?;
    spawn_list_build(move || {
        crate::test_runner::lang_rust::llvm_cov::build_current_rust_test_executable_index(
            &job.repo_root,
            &[],
            &job.extra,
            job.jobs,
        )
        .map(|_| ())
    });
    Ok(true)
}

fn join_started() -> Result<(), String> {
    let Some(handle) = HANDLE.with(|slot| slot.borrow_mut().take()) else {
        return Ok(());
    };
    match handle.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err),
        Err(_) => Err("error: kiss test: rust list-build panicked".to_string()),
    }
}

#[cfg(test)]
pub(crate) fn set_list_build_test_hook(hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = hook);
}

#[cfg(test)]
mod rust_list_build_test {
    use super::{
        covering_population_list_build_done, install_job, overlap_with_discover,
        set_list_build_test_hook, should_overlap,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn overlap_requires_cargo_toml_and_skips_nested_nextest() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!should_overlap(tmp.path()));
        std::fs::write(tmp.path().join("Cargo.toml"), "[package]\nname=\"t\"\n").unwrap();
        let nested =
            std::env::var_os("NEXTEST").is_some() || std::env::var_os("CARGO_LLVM_COV").is_some();
        assert_eq!(should_overlap(tmp.path()), !nested);
    }

    #[test]
    fn covering_discover_overlaps_started_list_build() {
        let tmp = tempfile::tempdir().unwrap();
        set_list_build_test_hook(None);
        let started = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&started);
        set_list_build_test_hook(Some(Arc::new(move || {
            flag.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(80));
        })));
        let _guard = install_job(tmp.path().to_path_buf(), Vec::new(), 1, false);
        overlap_with_discover(|| {
            let mark = Instant::now();
            while !started.load(Ordering::SeqCst) && mark.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(
                started.load(Ordering::SeqCst),
                "list-build must start before covering discover returns"
            );
            Ok(())
        })
        .expect("overlap discover");
        assert!(
            covering_population_list_build_done(),
            "covering overlap must mark list-build done"
        );
        set_list_build_test_hook(None);
    }

    #[test]
    fn list_build_source_does_not_install_live_status() {
        let llvm = include_str!("lang_rust/llvm_cov/mod.rs");
        let start = llvm
            .find("pub(crate) fn build_current_rust_test_executable_index")
            .expect("list-build function");
        let body = llvm[start..]
            .split("\npub(crate) fn ")
            .next()
            .expect("function body");
        assert!(
            !body.contains("install_live_rust_status_hook"),
            "list-build invocation must not install live status"
        );
        let begin = include_str!("rust_list_build.rs");
        let start = begin.find("fn begin()").expect("begin");
        let prod = begin[start..]
            .split("#[cfg(test)]")
            .next()
            .expect("begin body");
        assert!(
            !prod.contains("install_live"),
            "covering list-build must not install live status"
        );
        assert!(
            !prod.contains("tests_remaining"),
            "covering list-build must not print tests_remaining"
        );
    }
}
