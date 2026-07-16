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

pub(crate) const CHECK_RUNTIME_REFRESH_ACTIVE_ENV: &str = "KISS_CHECK_RUNTIME_REFRESH_ACTIVE";

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
    let _guard = lock_refresh(repo_root, "Rust")?;
    if load_rust_runtime_coverage(repo_root).is_ok() {
        return Ok(());
    }
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(repo_root, ignore)
            .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    if try_incremental_rust_runtime_coverage(repo_root, &selectors, jobs)? {
        return load_rust_runtime_coverage(repo_root)
            .map(|_| ())
            .map_err(|err| CoverageRefreshError::validation("Rust", err));
    }
    eprintln!(
        "kiss check: refreshing Rust runtime coverage ({} tests)",
        selectors.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let summary = crate::test_runner::runners::run_rust_llvm_cov_selectors(
        repo_root,
        &selectors,
        &[],
        false,
        jobs,
        Some(selectors.clone()),
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
    if summary.exit_code != 0 {
        return Err(CoverageRefreshError::TestExecution {
            language: "Rust",
            total: summary.total,
            failed: summary.failed,
            exit_code: summary.exit_code,
        });
    }
    load_rust_runtime_coverage(repo_root)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Rust", err))
}

fn try_incremental_rust_runtime_coverage(
    repo_root: &Path,
    selectors: &[String],
    jobs: usize,
) -> Result<bool, CoverageRefreshError> {
    let current_identity =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &[],
        )
        .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let cache_root = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(repo_root);
    let Some(prior) = rust_llvm_cov_runner::load_reusable_prior_population_state(
        &cache_root,
        repo_root,
        Some(selectors),
        &current_identity.selection_context_fingerprint,
    ) else {
        return Ok(false);
    };
    match rust_llvm_cov_runner::reusable_snapshot_delta(
        repo_root,
        &prior.ordinary_source_digests,
        &current_identity.ordinary_source_digests,
    ) {
        rust_llvm_cov_runner::RustSnapshotDelta::Unchanged => return Ok(false),
        rust_llvm_cov_runner::RustSnapshotDelta::StructuralChange => return Ok(false),
        rust_llvm_cov_runner::RustSnapshotDelta::Modified(_) => {}
    }
    let Some(prior_entries) =
        rust_llvm_cov_runner::load_reusable_prior_selector_entries(&cache_root, &prior)
    else {
        return Ok(false);
    };
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        repo_root,
        selectors,
        &[],
        jobs,
    )
    .map_err(|err| CoverageRefreshError::discovery("Rust", err))?;
    let current_binary_digest = build
        .index
        .test_binaries
        .iter()
        .map(|binary| (binary.id.clone(), binary.digest.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let (retained, invalid) = classify_incremental_rust_selectors(
        selectors,
        &prior,
        &prior_entries,
        &build.index.selector_binary_ids,
        &current_binary_digest,
    );
    eprintln!(
        "kiss check: incrementally refreshing Rust runtime coverage ({} reused, {} rerun)",
        retained.len(),
        invalid.len()
    );
    if !invalid.is_empty() {
        let _refresh_env = ScopedRefreshEnvGuard::set();
        let summary = crate::test_runner::runners::run_rust_llvm_cov_selectors(
            repo_root,
            &invalid,
            &[],
            false,
            jobs,
            None,
        )
        .map_err(|err| CoverageRefreshError::publication("Rust", err))?;
        if summary.exit_code != 0 {
            return Err(CoverageRefreshError::TestExecution {
                language: "Rust",
                total: summary.total,
                failed: summary.failed,
                exit_code: summary.exit_code,
            });
        }
    }
    rust_llvm_cov_runner::publish_incremental_derived_state(
        &build.request,
        &build.tools,
        &build.identity,
        rust_llvm_cov_runner::IncrementalPublishPlan {
            prior_generation: &prior.generation_fingerprint,
            selectors,
            retained_selectors: &retained,
            expected_selector_binaries: &build.index.selector_binary_ids,
            test_binaries: &build.index.test_binaries,
        },
    )
    .map_err(|err| CoverageRefreshError::publication("Rust", format!("{err:?}")))?;
    Ok(true)
}

fn classify_incremental_rust_selectors(
    selectors: &[String],
    prior: &rust_llvm_cov_runner::RustPopulationState,
    prior_entries: &std::collections::BTreeMap<
        String,
        rust_llvm_cov_runner::RustReusableSelectorEntry,
    >,
    current_selector_binaries: &std::collections::BTreeMap<String, Vec<String>>,
    current_binary_digest: &std::collections::BTreeMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut retained = Vec::new();
    let mut invalid = Vec::new();
    for selector in selectors {
        let Some(entry) = prior_entries.get(selector) else {
            invalid.push(selector.clone());
            continue;
        };
        let current_ids = current_selector_binaries
            .get(selector)
            .cloned()
            .unwrap_or_default();
        if entry.status != rpytest_runner::TestStatus::Passed
            || entry.test_binary_ids != current_ids
            || current_ids.iter().any(|id| {
                let prior_digest = prior.test_binaries.get(id).map(|binary| &binary.digest);
                let current_digest = current_binary_digest.get(id);
                prior_digest != current_digest
            })
        {
            invalid.push(selector.clone());
        } else {
            retained.push(selector.clone());
        }
    }
    (retained, invalid)
}

#[cfg(test)]
#[path = "check_runtime_refresh_test.rs"]
mod tests;
