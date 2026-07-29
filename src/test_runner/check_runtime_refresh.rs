use std::fmt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::Mutex;

use fs2::FileExt;

use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, RuntimeCoverageLoadError, load_python_runtime_coverage,
    load_rust_runtime_coverage,
};
use crate::test_runner::python_coverage_index::{
    publish_python_derived_state_with_filter,
    repo_relative_coverage_file as python_repo_relative_coverage_file,
};

#[path = "check_runtime_refresh_repair.rs"]
mod check_runtime_refresh_repair;
use check_runtime_refresh_repair::try_repair_rust_check_aggregate_labeled;
#[cfg(test)]
pub(crate) use check_runtime_refresh_repair::{
    CheckAggregateRepairDecision, classify_check_aggregate_repair,
};

#[path = "check_runtime_refresh_apply.rs"]
mod check_runtime_refresh_apply;
pub(crate) use check_runtime_refresh_apply::finalize_population_summary_labeled;
#[cfg(test)]
pub(crate) use check_runtime_refresh_apply::{
    apply_identity_only_repair, apply_rerun_repair, finalize_population_summary,
};

pub(crate) const COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV: &str =
    "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoverageRefreshStats {
    pub(crate) rust_test_instances: usize,
    pub(crate) rust_aggregate_binaries: usize,
    pub(crate) rust_aggregate_exports: usize,
    pub(crate) rust_identity_only_repair: bool,
    pub(crate) rust_full_refresh: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CoverageRefreshError {
    Lock {
        language: &'static str,
        reason: String,
    },
    Discovery {
        language: &'static str,
        reason: String,
    },
    TestExecution {
        language: &'static str,
        total: usize,
        failed: usize,
        exit_code: i32,
    },
    Publication {
        language: &'static str,
        reason: String,
    },
    PostRefreshValidation {
        language: &'static str,
        reason: String,
    },
}

impl CoverageRefreshError {
    pub(crate) fn lock(language: &'static str, err: impl ToString) -> Self {
        Self::Lock {
            language,
            reason: err.to_string(),
        }
    }

    pub(crate) fn discovery(language: &'static str, err: impl ToString) -> Self {
        Self::Discovery {
            language,
            reason: err.to_string(),
        }
    }

    pub(crate) fn publication(language: &'static str, err: impl ToString) -> Self {
        Self::Publication {
            language,
            reason: err.to_string(),
        }
    }

    pub(crate) fn validation(language: &'static str, err: RuntimeCoverageLoadError) -> Self {
        Self::PostRefreshValidation {
            language,
            reason: err.reason,
        }
    }
}

impl fmt::Display for CoverageRefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverageRefreshError::Lock { language, reason } => write!(
                f,
                "error: kiss cov: failed to refresh {language} runtime line coverage during lock acquisition: {reason}"
            ),
            CoverageRefreshError::Discovery { language, reason } => write!(
                f,
                "error: kiss cov: failed to refresh {language} runtime line coverage during test discovery: {reason}"
            ),
            CoverageRefreshError::TestExecution {
                language,
                total,
                failed,
                exit_code,
            } => write!(
                f,
                "error: kiss cov: failed to refresh {language} runtime line coverage because the population test run failed ({failed}/{total} tests failed, exit code {exit_code})"
            ),
            CoverageRefreshError::Publication { language, reason } => write!(
                f,
                "error: kiss cov: failed to refresh {language} runtime line coverage during publication: {reason}"
            ),
            CoverageRefreshError::PostRefreshValidation { language, reason } => write!(
                f,
                "error: kiss cov: failed to refresh {language} runtime line coverage during post-refresh validation: {reason}"
            ),
        }
    }
}

pub(crate) fn ensure_check_runtime_coverage(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    match (required.python, required.rust) {
        (true, true) => refresh_python_and_rust_parallel(repo_root, ignore, jobs),
        (true, false) => ensure_python_runtime_coverage(repo_root, ignore, jobs),
        (false, true) => ensure_rust_runtime_coverage(repo_root, ignore, jobs),
        (false, false) => Ok(()),
    }
}

fn refresh_python_and_rust_parallel(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    // Per-language file locks allow overlap. Refcounted ScopedRefreshEnvGuard makes
    // the process-wide refresh env marker safe across these two threads.
    std::thread::scope(|scope| {
        let python = scope.spawn(|| ensure_python_runtime_coverage(repo_root, ignore, jobs));
        let rust = ensure_rust_runtime_coverage(repo_root, ignore, jobs);
        let python = python
            .join()
            .unwrap_or_else(|_| Err(CoverageRefreshError::publication(
                "Python",
                "python refresh thread panicked",
            )));
        rust.and(python)
    })
}

struct RefreshLockGuard {
    _file: File,
}

pub(crate) struct ScopedRefreshEnvGuard {
    _private: (),
}

/// Depth + saved prior env value, guarded together so concurrent `set`/`Drop`
/// cannot observe a bumped depth before the process env marker is written.
static REFRESH_ENV_STATE: Mutex<(usize, Option<Option<std::ffi::OsString>>)> =
    Mutex::new((0, None));

impl ScopedRefreshEnvGuard {
    pub(crate) fn set() -> Self {
        let mut state = REFRESH_ENV_STATE
            .lock()
            .expect("refresh env state lock");
        if state.0 == 0 {
            state.1 = Some(std::env::var_os(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV));
            // SAFETY: process-wide marker for nested kiss/check children during refresh.
            // Depth counting allows concurrent Python+Rust refresh threads to share it.
            unsafe { std::env::set_var(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV, "1") };
        }
        state.0 += 1;
        Self { _private: () }
    }
}

impl Drop for ScopedRefreshEnvGuard {
    fn drop(&mut self) {
        let mut state = REFRESH_ENV_STATE
            .lock()
            .expect("refresh env state lock");
        state.0 = state.0.saturating_sub(1);
        if state.0 == 0 {
            let old = state.1.take().flatten();
            restore_refresh_active_env(old);
        }
    }
}

pub(crate) fn restore_refresh_active_env(old: Option<std::ffi::OsString>) {
    let key = COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV;
    match old {
        // SAFETY: restores the guard set by `ScopedRefreshEnvGuard::set`.
        Some(value) => unsafe { std::env::set_var(key, value) },
        // SAFETY: restores the absence of the guard set above.
        None => unsafe { std::env::remove_var(key) },
    }
}

fn lock_refresh(
    repo_root: &Path,
    language: &'static str,
) -> Result<RefreshLockGuard, CoverageRefreshError> {
    let path = repo_root
        .join(".kiss")
        .join("check_runtime_coverage_locks")
        .join(format!("{language}.lock"));
    let parent = path
        .parent()
        .ok_or_else(|| CoverageRefreshError::lock(language, "lock path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(|err| CoverageRefreshError::lock(language, err))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|err| CoverageRefreshError::lock(language, err))?;
    file.lock_exclusive()
        .map_err(|err| CoverageRefreshError::lock(language, err))?;
    Ok(RefreshLockGuard { _file: file })
}

fn ensure_python_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Python")?;
    if load_python_runtime_coverage(repo_root).is_ok() {
        return Ok(());
    }
    let selectors =
        crate::test_runner::runners::enumerate_workspace_python_selectors(repo_root, ignore)
            .map_err(|err| CoverageRefreshError::discovery("Python", err))?;
    eprintln!(
        "kiss cov: refreshing Python runtime coverage ({} tests)",
        selectors.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let summary =
        crate::test_runner::runners::run_rslip_selectors(repo_root, &selectors, &[], false, jobs)
            .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    if summary.exit_code != 0 {
        return Err(CoverageRefreshError::TestExecution {
            language: "Python",
            total: summary.total,
            failed: summary.failed,
            exit_code: summary.exit_code,
        });
    }
    publish_python_derived_state_with_filter(
        repo_root,
        Some(&selectors),
        &[],
        |path, repo_root| {
            python_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
        },
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    load_python_runtime_coverage(repo_root)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Python", err))
}

fn ensure_rust_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    ensure_rust_runtime_coverage_with_stats_labeled(repo_root, ignore, jobs, "kiss cov").map(|_| ())
}

fn ensure_rust_runtime_coverage_with_stats_labeled(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Rust")?;
    match load_rust_runtime_coverage(repo_root, ignore) {
        Ok(_) => Ok(CoverageRefreshStats::default()),
        Err(_) => {
            let selectors =
                crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
                    .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
            if let Some(stats) =
                try_repair_rust_check_aggregate_labeled(repo_root, ignore, &selectors, jobs, caller_label)?
            {
                return Ok(stats);
            }
            refresh_full_rust_check_aggregate_labeled(repo_root, ignore, &selectors, jobs, caller_label)
        }
    }
}

/// Shared Rust ensure/refresh entry point used by both `kiss cov` and cold
/// non-force `kiss test` population when `extra` is empty.
///
/// Runs the lock / load / repair / full-refresh body against repo-local
/// `./.kiss` coverage artifacts. Returns a `SelectorExecutionSummary` with
/// cached-hit accounting when coverage was already loadable, or the real batch
/// summary after a full refresh.
pub(crate) fn ensure_rust_runtime_coverage_shared(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    caller_label: &str,
) -> Result<crate::test_runner::runners::SelectorExecutionSummary, CoverageRefreshError> {
    if load_rust_runtime_coverage(repo_root, ignore).is_ok() {
        let rust_batch_cache_hits =
            crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
                .unwrap_or_default()
                .len();
        return Ok(crate::test_runner::runners::SelectorExecutionSummary {
            rust_batch_cache_hits,
            ..Default::default()
        });
    }
    let stats = ensure_rust_runtime_coverage_with_stats_labeled(repo_root, ignore, jobs, caller_label)?;
    Ok(crate::test_runner::runners::SelectorExecutionSummary {
        rust_test_instances: stats.rust_test_instances,
        rust_aggregate_binaries: stats.rust_aggregate_binaries,
        rust_aggregate_exports: stats.rust_aggregate_exports,
        ..Default::default()
    })
}

fn refresh_full_rust_check_aggregate_labeled(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
    caller_label: &str,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "{caller_label}: refreshing Rust runtime coverage ({} tests)",
        selectors.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let summary = crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors(
        repo_root,
        selectors,
        &[],
        jobs,
        None,
        None,
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
    finalize_population_summary_labeled(repo_root, ignore, &summary, true, caller_label)
}

#[cfg(test)]
pub(crate) fn try_repair_rust_check_aggregate(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
) -> Result<Option<CoverageRefreshStats>, CoverageRefreshError> {
    try_repair_rust_check_aggregate_labeled(repo_root, ignore, selectors, jobs, "kiss cov")
}

#[cfg(test)]
#[path = "check_runtime_refresh_test.rs"]
mod tests;

#[cfg(test)]
#[path = "check_runtime_refresh_apply_test.rs"]
mod apply_tests;
