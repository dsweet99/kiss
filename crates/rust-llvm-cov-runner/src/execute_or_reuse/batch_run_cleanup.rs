use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::execute_or_reuse::batch_process_tree::BatchScopeInterruptGuard;
use crate::{RustLlvmCovError, batch_result::RustCoverageBatchResult};

pub type CurrentRunCleanupFn = fn(&Path, &Path) -> io::Result<()>;

#[derive(Clone, Copy, Debug)]
pub struct CurrentRunCleanup {
    remove: CurrentRunCleanupFn,
}

impl CurrentRunCleanup {
    pub fn default_cleanup() -> Self {
        Self {
            remove: remove_current_run_directory,
        }
    }

    #[cfg(test)]
    pub fn injecting(remove: CurrentRunCleanupFn) -> Self {
        Self { remove }
    }

    pub fn remove(&self, cache_root: &Path, run_root: &Path) -> io::Result<()> {
        (self.remove)(cache_root, run_root)
    }
}

impl Default for CurrentRunCleanup {
    fn default() -> Self {
        Self::default_cleanup()
    }
}

pub struct CurrentRunLifecycleGuard {
    cache_root: PathBuf,
    run_root: PathBuf,
    cleanup: CurrentRunCleanup,
    cleaned: Cell<bool>,
}

impl CurrentRunLifecycleGuard {
    #[cfg(test)]
    pub fn new(cache_root: PathBuf, run_root: PathBuf) -> Self {
        Self::with_cleanup(cache_root, run_root, CurrentRunCleanup::default())
    }

    pub fn with_cleanup(
        cache_root: PathBuf,
        run_root: PathBuf,
        cleanup: CurrentRunCleanup,
    ) -> Self {
        Self {
            cache_root,
            run_root,
            cleanup,
            cleaned: Cell::new(false),
        }
    }

    pub fn cleanup(&self) -> Option<io::Error> {
        if self.cleaned.get() {
            return None;
        }
        self.cleaned.set(true);
        let run_err = self.cleanup.remove(&self.cache_root, &self.run_root).err();
        let kiss_profraw = crate::kiss_profraw::kiss_profraw_from_cache_root(&self.cache_root);
        let profraw_err = crate::kiss_profraw::cleanup_kiss_profraw(&kiss_profraw).err();

        let orphan_err = crate::kiss_profraw::repo_root_from_cache_root(&self.cache_root)
            .and_then(|root| crate::kiss_profraw::sweep_orphan_default_profraw(&root).err());
        fold_cleanup_errors([run_err, profraw_err, orphan_err])
    }
}

fn fold_cleanup_errors(errors: [Option<io::Error>; 3]) -> Option<io::Error> {
    let mut combined: Option<io::Error> = None;
    for err in errors.into_iter().flatten() {
        combined = Some(match combined {
            None => err,
            Some(prior) => io::Error::new(prior.kind(), format!("{prior}; {err}")),
        });
    }
    combined
}

impl Drop for CurrentRunLifecycleGuard {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

pub struct FreshBatchRunScope {
    stale_cleanup_error: Option<io::Error>,
    lifecycle: CurrentRunLifecycleGuard,
    _interrupt_guard: BatchScopeInterruptGuard,
}

impl FreshBatchRunScope {
    pub fn begin(
        cache_root: &Path,
        run_root: PathBuf,
        cleanup: CurrentRunCleanup,
    ) -> io::Result<Self> {
        let interrupt_guard = BatchScopeInterruptGuard::install()?;
        let stale_cleanup_error = super::remove_stale_run_directories(cache_root, &run_root).err();
        Ok(Self {
            stale_cleanup_error,
            lifecycle: CurrentRunLifecycleGuard::with_cleanup(
                cache_root.to_path_buf(),
                run_root,
                cleanup,
            ),
            _interrupt_guard: interrupt_guard,
        })
    }

    pub fn begin_with_layout(
        cache_root: &Path,
        plan: &crate::plan::batch_plan::RustCoverageBatchPlan,
        cleanup: CurrentRunCleanup,
    ) -> io::Result<Self> {
        let run_root = plan
            .generated_config
            .parent()
            .ok_or_else(|| io::Error::other("generated nextest config path has no parent"))?
            .to_path_buf();
        fs::create_dir_all(&run_root)?;
        let scope = Self::begin(cache_root, run_root, cleanup)?;
        fs::create_dir_all(&plan.target_runner_output_dir)?;
        let kiss_profraw = crate::kiss_profraw::kiss_profraw_from_cache_root(cache_root);
        crate::kiss_profraw::ensure_kiss_profraw(&kiss_profraw)?;
        if let Some(repo_root) = crate::kiss_profraw::repo_root_from_cache_root(cache_root) {
            crate::kiss_profraw::sweep_orphan_default_profraw(&repo_root)?;
        }
        Ok(scope)
    }

    pub fn finish<T>(self, outcome: Result<T, RustLlvmCovError>) -> Result<T, RustLlvmCovError> {
        let current_cleanup_error = self.lifecycle.cleanup();
        match outcome {
            Ok(value) => {
                if let Some(err) =
                    sole_cleanup_error(self.stale_cleanup_error, current_cleanup_error)
                {
                    return Err(err.into());
                }
                Ok(value)
            }
            Err(primary) => Err(combine_execution_error(
                primary,
                self.stale_cleanup_error,
                current_cleanup_error,
            )),
        }
    }

    pub fn finish_batch_result(
        self,
        result: RustCoverageBatchResult,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
        let current_cleanup_error = self.lifecycle.cleanup();
        finalize_batch_result(result, self.stale_cleanup_error, current_cleanup_error)
    }
}

pub fn validate_run_directory_under_cache_root(
    cache_root: &Path,
    run_root: &Path,
) -> io::Result<()> {
    if run_root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory with parent traversal: {}",
                run_root.display()
            ),
        ));
    }
    let runs_root = cache_root.join("runs");
    let relative = run_root.strip_prefix(&runs_root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory outside cache runs root: {}",
                run_root.display()
            ),
        )
    })?;
    if relative.components().count() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory that is not a direct runs child: {}",
                run_root.display()
            ),
        ));
    }
    if runs_root.is_symlink() || run_root.is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove symlinked run directory: {}",
                run_root.display()
            ),
        ));
    }
    let canonical_cache_root = cache_root.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory with unresolvable cache root {}: {err}",
                cache_root.display()
            ),
        )
    })?;
    let canonical_runs_root = runs_root.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory with unresolvable runs root {}: {err}",
                runs_root.display()
            ),
        )
    })?;
    if canonical_runs_root != canonical_cache_root.join("runs") {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory with escaped runs root: {}",
                runs_root.display()
            ),
        ));
    }
    let canonical_run_root = run_root.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory with unresolvable path {}: {err}",
                run_root.display()
            ),
        )
    })?;
    let canonical_relative = canonical_run_root
        .strip_prefix(&canonical_runs_root)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to remove run directory outside canonical runs root: {}",
                    run_root.display()
                ),
            )
        })?;
    if canonical_relative.components().count() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "refusing to remove run directory that is not a direct canonical runs child: {}",
                run_root.display()
            ),
        ));
    }
    Ok(())
}

pub fn remove_current_run_directory(cache_root: &Path, run_root: &Path) -> io::Result<()> {
    validate_run_directory_under_cache_root(cache_root, run_root)?;
    if run_root.is_dir() {
        fs::remove_dir_all(run_root)?;
    }
    Ok(())
}

pub fn append_cleanup_error(primary: RustLlvmCovError, cleanup: io::Error) -> RustLlvmCovError {
    match primary {
        RustLlvmCovError::InvalidRequest(message) => {
            RustLlvmCovError::InvalidRequest(format!("{message}; cleanup failed: {cleanup}"))
        }
        RustLlvmCovError::Interrupted => RustLlvmCovError::Interrupted,
        other => RustLlvmCovError::InvalidRequest(format!("{other:?}; cleanup failed: {cleanup}")),
    }
}

pub fn combine_execution_error(
    primary: RustLlvmCovError,
    stale_cleanup: Option<io::Error>,
    current_cleanup: Option<io::Error>,
) -> RustLlvmCovError {
    let mut combined = primary;
    for cleanup in [stale_cleanup, current_cleanup].into_iter().flatten() {
        combined = append_cleanup_error(combined, cleanup);
    }
    combined
}

pub fn finalize_batch_result(
    result: RustCoverageBatchResult,
    stale_cleanup: Option<io::Error>,
    current_cleanup: Option<io::Error>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    let cleanup_errors: Vec<io::Error> = [stale_cleanup, current_cleanup]
        .into_iter()
        .flatten()
        .collect();
    if cleanup_errors.is_empty() {
        return Ok(result);
    }
    if let Some(primary) = result.batch_error {
        let mut combined = primary;
        for cleanup in cleanup_errors {
            combined = append_cleanup_error(combined, cleanup);
        }
        return Ok(RustCoverageBatchResult {
            batch_error: Some(combined),
            ..result
        });
    }
    let mut cleanup_errors = cleanup_errors;
    let mut combined = RustLlvmCovError::Io(cleanup_errors.remove(0));
    for cleanup in cleanup_errors {
        combined = append_cleanup_error(combined, cleanup);
    }
    Ok(RustCoverageBatchResult {
        batch_error: Some(combined),
        ..result
    })
}

fn sole_cleanup_error(
    stale_cleanup: Option<io::Error>,
    current_cleanup: Option<io::Error>,
) -> Option<io::Error> {
    match (stale_cleanup, current_cleanup) {
        (None, None) => None,
        (Some(err), None) | (None, Some(err)) => Some(err),
        (Some(stale), Some(current)) => {
            Some(io::Error::new(stale.kind(), format!("{stale}; {current}")))
        }
    }
}

#[cfg(test)]
#[path = "batch_run_cleanup_test.rs"]
mod tests;
