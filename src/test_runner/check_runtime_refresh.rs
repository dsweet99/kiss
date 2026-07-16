use std::fmt;
use std::fs::{File, OpenOptions};
use std::path::Path;

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
use check_runtime_refresh_repair::{CheckAggregateRepairDecision, classify_check_aggregate_repair};

pub(crate) const CHECK_RUNTIME_REFRESH_ACTIVE_ENV: &str = "KISS_CHECK_RUNTIME_REFRESH_ACTIVE";

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
    fn lock(language: &'static str, err: impl ToString) -> Self {
        Self::Lock {
            language,
            reason: err.to_string(),
        }
    }

    fn discovery(language: &'static str, err: impl ToString) -> Self {
        Self::Discovery {
            language,
            reason: err.to_string(),
        }
    }

    fn publication(language: &'static str, err: impl ToString) -> Self {
        Self::Publication {
            language,
            reason: err.to_string(),
        }
    }

    fn validation(language: &'static str, err: RuntimeCoverageLoadError) -> Self {
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
                "error: kiss check: failed to refresh {language} runtime line coverage during lock acquisition: {reason}"
            ),
            CoverageRefreshError::Discovery { language, reason } => write!(
                f,
                "error: kiss check: failed to refresh {language} runtime line coverage during test discovery: {reason}"
            ),
            CoverageRefreshError::TestExecution {
                language,
                total,
                failed,
                exit_code,
            } => write!(
                f,
                "error: kiss check: failed to refresh {language} runtime line coverage because the population test run failed ({failed}/{total} tests failed, exit code {exit_code})"
            ),
            CoverageRefreshError::Publication { language, reason } => write!(
                f,
                "error: kiss check: failed to refresh {language} runtime line coverage during publication: {reason}"
            ),
            CoverageRefreshError::PostRefreshValidation { language, reason } => write!(
                f,
                "error: kiss check: failed to refresh {language} runtime line coverage during post-refresh validation: {reason}"
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
    if required.python {
        ensure_python_runtime_coverage(repo_root, ignore, jobs)?;
    }
    if required.rust {
        ensure_rust_runtime_coverage(repo_root, ignore, jobs)?;
    }
    Ok(())
}

struct RefreshLockGuard {
    _file: File,
}

struct ScopedRefreshEnvGuard {
    old: Option<std::ffi::OsString>,
}

impl ScopedRefreshEnvGuard {
    fn set() -> Self {
        let old = std::env::var_os(CHECK_RUNTIME_REFRESH_ACTIVE_ENV);
        // SAFETY: `kiss check` sets this process-wide guard only around its
        // synchronous population runner call, before waiting for child tests.
        unsafe { std::env::set_var(CHECK_RUNTIME_REFRESH_ACTIVE_ENV, "1") };
        Self { old }
    }
}

impl Drop for ScopedRefreshEnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => {
                // SAFETY: restores the guard set by `ScopedRefreshEnvGuard::set`.
                unsafe { std::env::set_var(CHECK_RUNTIME_REFRESH_ACTIVE_ENV, value) };
            }
            None => {
                // SAFETY: restores the absence of the guard set above.
                unsafe { std::env::remove_var(CHECK_RUNTIME_REFRESH_ACTIVE_ENV) };
            }
        }
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
        "kiss check: refreshing Python runtime coverage ({} tests)",
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
    ensure_rust_runtime_coverage_with_stats(repo_root, ignore, jobs).map(|_| ())
}

fn ensure_rust_runtime_coverage_with_stats(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Rust")?;
    if load_rust_runtime_coverage(repo_root, ignore).is_ok() {
        return Ok(CoverageRefreshStats::default());
    }
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
            .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    if let Some(stats) = try_repair_rust_check_aggregate(repo_root, ignore, &selectors, jobs)? {
        return Ok(stats);
    }
    refresh_full_rust_check_aggregate(repo_root, ignore, &selectors, jobs)
}

fn refresh_full_rust_check_aggregate(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "kiss check: refreshing Rust runtime coverage ({} tests)",
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
    eprintln!(
        "kiss check: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports={}",
        summary.rust_aggregate_binaries, summary.rust_aggregate_exports
    );
    if summary.exit_code != 0 {
        return Err(CoverageRefreshError::TestExecution {
            language: "Rust",
            total: summary.total,
            failed: summary.failed,
            exit_code: summary.exit_code,
        });
    }
    load_rust_runtime_coverage(repo_root, ignore)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
    Ok(CoverageRefreshStats {
        rust_test_instances: summary.rust_test_instances,
        rust_aggregate_binaries: summary.rust_aggregate_binaries,
        rust_aggregate_exports: summary.rust_aggregate_exports,
        rust_full_refresh: true,
        ..Default::default()
    })
}

fn try_repair_rust_check_aggregate(
    repo_root: &Path,
    ignore: &[String],
    selectors: &[String],
    jobs: usize,
) -> Result<Option<CoverageRefreshStats>, CoverageRefreshError> {
    let current_identity =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &[],
        )
        .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    let Some(prior) = rust_llvm_cov_runner::load_reusable_prior_check_aggregate(
        &cache_root,
        repo_root,
        selectors,
        &current_identity.selection_context_fingerprint,
    ) else {
        return Ok(None);
    };
    match rust_llvm_cov_runner::reusable_check_aggregate_delta(
        repo_root,
        &prior.ordinary_source_digests,
        &current_identity.ordinary_source_digests,
    ) {
        rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange => return Ok(None),
        rust_llvm_cov_runner::RustSnapshotDelta::Unchanged
        | rust_llvm_cov_runner::RustSnapshotDelta::Modified(_) => {}
    }
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        repo_root,
        selectors,
        &[],
        jobs,
    )
    .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let decision = classify_check_aggregate_repair(
        selectors,
        &prior,
        &build.index.selector_binary_ids,
        &build.index.test_binaries,
    );
    match decision {
        CheckAggregateRepairDecision::FullRefresh => Ok(None),
        CheckAggregateRepairDecision::IdentityOnly {
            retained_binary_line_maps,
        } => {
            let aggregate = rust_llvm_cov_runner::build_check_aggregate(
                &build.request,
                &build.identity,
                selectors,
                build.index.selector_binary_ids,
                &build.index.test_binaries,
                retained_binary_line_maps,
            )
            .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
            rust_llvm_cov_runner::publish_check_aggregate(&build.request, &aggregate)
                .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
            load_rust_runtime_coverage(repo_root, ignore)
                .map(|_| ())
                .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
            eprintln!(
                "kiss check: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports=0",
                aggregate.binaries.len()
            );
            Ok(Some(CoverageRefreshStats {
                rust_aggregate_binaries: aggregate.binaries.len(),
                rust_identity_only_repair: true,
                ..Default::default()
            }))
        }
        CheckAggregateRepairDecision::Rerun {
            rerun_selectors,
            replacement_binary_ids,
            retained_binary_line_maps,
        } => {
            eprintln!(
                "kiss check: incrementally refreshing Rust runtime coverage ({} tests, {} replacement binaries)",
                rerun_selectors.len(),
                replacement_binary_ids.len()
            );
            let _refresh_env = ScopedRefreshEnvGuard::set();
            let repair_publication = rust_llvm_cov_runner::CheckAggregateRepairPublication {
                selector_binary_ids: build.index.selector_binary_ids,
                test_binaries: build.index.test_binaries,
                retained_binary_line_maps,
            };
            let summary =
                crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors(
                    repo_root,
                    &rerun_selectors,
                    &[],
                    jobs,
                    Some(replacement_binary_ids),
                    Some(repair_publication),
                )
                .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
            eprintln!(
                "kiss check: refreshed Rust runtime coverage rust_aggregate_binaries={} rust_aggregate_exports={}",
                summary.rust_aggregate_binaries, summary.rust_aggregate_exports
            );
            if summary.exit_code != 0 {
                return Err(CoverageRefreshError::TestExecution {
                    language: "Rust",
                    total: summary.total,
                    failed: summary.failed,
                    exit_code: summary.exit_code,
                });
            }
            load_rust_runtime_coverage(repo_root, ignore)
                .map(|_| ())
                .map_err(|err| CoverageRefreshError::validation("Rust", err))?;
            Ok(Some(CoverageRefreshStats {
                rust_test_instances: summary.rust_test_instances,
                rust_aggregate_binaries: summary.rust_aggregate_binaries,
                rust_aggregate_exports: summary.rust_aggregate_exports,
                ..Default::default()
            }))
        }
    }
}

#[cfg(test)]
#[path = "check_runtime_refresh_test.rs"]
mod tests;
