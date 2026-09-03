use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use fs2::FileExt;

use crate::test_runner::check_line_coverage::{
    RequiredCoverageLanguages, RuntimeCoverageLoadError, load_rust_runtime_coverage,
};

#[path = "check_runtime_refresh_repair.rs"]
mod check_runtime_refresh_repair;
use check_runtime_refresh_repair::try_repair_rust_check_aggregate_labeled;
#[cfg(test)]
pub(crate) use check_runtime_refresh_repair::{
    CheckAggregateRepairDecision, classify_check_aggregate_repair,
    classify_check_aggregate_repair_with_replacements,
};

#[path = "check_runtime_refresh_apply.rs"]
mod check_runtime_refresh_apply;
use check_runtime_refresh_apply::finalize_population_summary_labeled;
#[cfg(test)]
pub(crate) use check_runtime_refresh_apply::{
    RerunRepairArgs, apply_identity_only_repair, apply_rerun_repair, finalize_population_summary,
};

#[path = "check_runtime_refresh_python.rs"]
mod check_runtime_refresh_python;
use check_runtime_refresh_python::ensure_python_runtime_coverage;

#[path = "check_runtime_refresh_types.rs"]
mod check_runtime_refresh_types;
pub(crate) use check_runtime_refresh_types::CoverageRuntimeRefresh;
use check_runtime_refresh_types::{PythonRuntimeRefresh, RustRuntimeRefresh};

#[cfg(test)]
#[path = "check_runtime_refresh_python_test.rs"]
mod python_refresh_tests;

pub(crate) const COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV: &str = "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE";

pub(crate) fn test_runner_stdout_enabled() -> bool {
    std::env::var_os(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV).is_none()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LanguageRefreshStats {
    pub(crate) test_instances: usize,
    pub(crate) aggregate_binaries: usize,
    pub(crate) aggregate_exports: usize,
    pub(crate) identity_only_repair: bool,
    pub(crate) full_refresh: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoverageRefreshStats {
    pub(crate) by_language: crate::test_runner::language_keyed::LanguageKeyed<LanguageRefreshStats>,
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
                "error: kiss test: failed to refresh {language} runtime line coverage during lock acquisition: {reason}"
            ),
            CoverageRefreshError::Discovery { language, reason } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage during test discovery: {reason}"
            ),
            CoverageRefreshError::TestExecution {
                language,
                total,
                failed,
                exit_code,
            } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage because the population test run failed ({failed}/{total} tests failed, exit code {exit_code})"
            ),
            CoverageRefreshError::Publication { language, reason } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage during publication: {reason}"
            ),
            CoverageRefreshError::PostRefreshValidation { language, reason } => write!(
                f,
                "error: kiss test: failed to refresh {language} runtime line coverage during post-refresh validation: {reason}"
            ),
        }
    }
}

pub(crate) fn ensure_check_runtime_coverage(
    repo_root: &Path,
    required: RequiredCoverageLanguages,
    ignore: &[String],
    jobs: usize,
    pytest_args: &[String],
    gate: &kiss::GateConfig,
) -> Result<(), CoverageRefreshError> {
    match (required.python, required.rust) {
        (true, true) => {
            refresh_python_and_rust_parallel(repo_root, ignore, jobs, pytest_args, gate).map(|_| ())
        }
        (true, false) => {
            let refresh = PythonRuntimeRefresh;
            debug_assert_eq!(refresh.language(), kiss::Language::Python);
            refresh
                .ensure(repo_root, ignore, jobs, pytest_args, gate)
                .map(|_| ())
        }
        (false, true) => {
            let refresh = RustRuntimeRefresh;
            debug_assert_eq!(refresh.language(), kiss::Language::Rust);
            refresh
                .ensure(repo_root, ignore, jobs, pytest_args, gate)
                .map(|_| ())
        }
        (false, false) => Ok(()),
    }
}

fn refresh_python_and_rust_parallel(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    pytest_args: &[String],
    gate: &kiss::GateConfig,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    std::thread::scope(|scope| {
        let python = scope
            .spawn(|| ensure_python_runtime_coverage(repo_root, ignore, jobs, pytest_args, gate));
        let rust = ensure_rust_runtime_coverage(repo_root, ignore, jobs, gate)?;
        let python = python.join().unwrap_or_else(|_| {
            Err(CoverageRefreshError::publication(
                "Python",
                "python refresh thread panicked",
            ))
        })?;
        Ok(CoverageRefreshStats {
            by_language: crate::test_runner::language_keyed::LanguageKeyed {
                python: python.by_language.python,
                rust: rust.by_language.rust,
            },
        })
    })
}

#[derive(Debug)]
pub(super) struct RefreshLockGuard {
    _file: File,
}

pub(crate) struct ScopedRefreshEnvGuard {
    _private: (),
}

static REFRESH_ENV_STATE: Mutex<(usize, Option<Option<std::ffi::OsString>>)> =
    Mutex::new((0, None));

impl ScopedRefreshEnvGuard {
    pub(crate) fn set() -> Self {
        let mut state = REFRESH_ENV_STATE.lock().expect("refresh env state lock");
        if state.0 == 0 {
            state.1 = Some(std::env::var_os(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV));

            unsafe { std::env::set_var(COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV, "1") };
        }
        state.0 += 1;
        Self { _private: () }
    }
}

impl Drop for ScopedRefreshEnvGuard {
    fn drop(&mut self) {
        let mut state = REFRESH_ENV_STATE.lock().expect("refresh env state lock");
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
        Some(value) => unsafe { std::env::set_var(key, value) },

        None => unsafe { std::env::remove_var(key) },
    }
}

pub(super) fn lock_refresh(
    repo_root: &Path,
    language: &'static str,
) -> Result<RefreshLockGuard, CoverageRefreshError> {
    lock_refresh_for(repo_root, language, Duration::from_secs(30))
}

fn lock_refresh_for(
    repo_root: &Path,
    language: &'static str,
    timeout: Duration,
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
    let mut reported_wait = false;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => break,
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if !reported_wait {
                    crate::test_runner::emit_test_progress(&format!(
                        "kiss test: waiting for {language} runtime coverage refresh"
                    ));
                    reported_wait = true;
                }
                let now = std::time::Instant::now();
                if now >= deadline {
                    return Err(CoverageRefreshError::lock(
                        language,
                        std::io::Error::new(
                            ErrorKind::TimedOut,
                            format!("timed out after {}s", timeout.as_secs_f64()),
                        ),
                    ));
                }
                std::thread::sleep(
                    deadline
                        .saturating_duration_since(now)
                        .min(Duration::from_millis(250)),
                );
            }
            Err(err) => return Err(CoverageRefreshError::lock(language, err)),
        }
    }
    Ok(RefreshLockGuard { _file: file })
}

pub(super) fn ensure_rust_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    gate: &kiss::GateConfig,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    ensure_rust_runtime_coverage_with_stats_labeled(repo_root, ignore, jobs, "kiss test", gate)
}

fn ensure_rust_runtime_coverage_with_stats_labeled(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    caller_label: &str,
    gate: &kiss::GateConfig,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Rust")?;
    if load_rust_runtime_coverage(repo_root, ignore, gate).is_ok() {
        return Ok(CoverageRefreshStats::default());
    }
    let request = crate::test_runner::ensure_runtime::ensure_request_for_all(
        repo_root,
        ignore,
        jobs,
        Some(kiss::Language::Rust),
        false,
        gate.clone(),
        vec![],
    )
    .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;

    if let Some(stats) = try_repair_rust_check_aggregate_labeled(
        repo_root,
        ignore,
        &request.planned.rust,
        jobs,
        caller_label,
    )? {
        return Ok(stats);
    }
    eprintln!(
        "{caller_label}: refreshing Rust runtime coverage ({} tests)",
        request.planned.rust.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let result = crate::test_runner::ensure_runtime::ensure_languages_runtime(&request)
        .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
    let summary = result.rust().map(|r| r.summary.clone()).unwrap_or_default();
    finalize_population_summary_labeled(repo_root, ignore, &summary, true, caller_label)
}

#[allow(dead_code)]
pub(crate) fn ensure_rust_runtime_coverage_shared(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    caller_label: &str,
    gate: &kiss::GateConfig,
) -> Result<crate::test_runner::runners::SelectorExecutionSummary, CoverageRefreshError> {
    if load_rust_runtime_coverage(repo_root, ignore, gate).is_ok() {
        let rust_batch_cache_hits =
            crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
                .unwrap_or_default()
                .len();
        return Ok(crate::test_runner::runners::SelectorExecutionSummary {
            total: rust_batch_cache_hits,
            cache_hits: rust_batch_cache_hits,
            rust_batch_cache_hits,
            ..Default::default()
        });
    }
    let stats = ensure_rust_runtime_coverage_with_stats_labeled(
        repo_root,
        ignore,
        jobs,
        caller_label,
        gate,
    )?;
    Ok(crate::test_runner::runners::SelectorExecutionSummary {
        rust_test_instances: stats.by_language.rust.test_instances,
        rust_aggregate_binaries: stats.by_language.rust.aggregate_binaries,
        rust_aggregate_exports: stats.by_language.rust.aggregate_exports,
        ..Default::default()
    })
}

#[cfg(test)]
pub(crate) fn try_repair_rust_check_aggregate(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
) -> Result<Option<CoverageRefreshStats>, CoverageRefreshError> {
    try_repair_rust_check_aggregate_labeled(repo_root, ignore, selectors, jobs, "kiss test")
}

#[cfg(test)]
#[path = "check_runtime_refresh_test.rs"]
mod tests;

#[cfg(test)]
#[path = "check_runtime_refresh_apply_test.rs"]
mod apply_tests;

#[cfg(test)]
#[path = "check_runtime_refresh_apply_b_test.rs"]
mod apply_b_tests;
