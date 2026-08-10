//! Python incomplete-generation repair and full cold refresh for `kiss cov`.

use std::path::Path;

use super::{
    CoverageRefreshError, ScopedRefreshEnvGuard, lock_refresh,
};
use crate::test_runner::check_line_coverage::{
    RuntimeCoverageLoadError, load_python_runtime_coverage,
};
use crate::test_runner::python_coverage_index::{
    publish_python_derived_state_with_filter,
    repo_relative_coverage_file as python_repo_relative_coverage_file,
};
use crate::test_runner::runners::SelectorExecutionSummary;

pub(super) fn ensure_python_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Python")?;
    match load_python_runtime_coverage(repo_root) {
        Ok(_) => return Ok(()),
        Err(err) if !err.problem_selectors.is_empty() => {
            return repair_incomplete_python_generation(
                repo_root,
                &err.problem_selectors,
                jobs,
            );
        }
        Err(_) => {}
    }
    refresh_full_python_runtime_coverage(repo_root, ignore, jobs)
}

fn refresh_full_python_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    let python_extra = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    let selectors = crate::test_runner::runners::enumerate_workspace_python_selectors(
        repo_root,
        ignore,
        &python_extra,
    )
    .map_err(|err| CoverageRefreshError::discovery("Python", err))?;
    eprintln!(
        "kiss cov: refreshing Python runtime coverage ({} tests)",
        selectors.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let summary = crate::test_runner::runners::run_rslip_selectors(
        repo_root,
        &selectors,
        &python_extra,
        false,
        &[],
        jobs,
        None,
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    if summary.exit_code != 0 {
        return Err(execution_error(&summary));
    }
    publish_python_derived_state_with_filter(
        repo_root,
        Some(&selectors),
        &python_extra,
        |path, repo_root| {
            python_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
        },
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
    load_python_runtime_coverage(repo_root)
        .map(|_| ())
        .map_err(|err| CoverageRefreshError::validation("Python", err))
}

fn repair_incomplete_python_generation(
    repo_root: &Path,
    problem_selectors: &[String],
    jobs: usize,
) -> Result<(), CoverageRefreshError> {
    let python_extra = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    eprintln!(
        "kiss cov: repairing incomplete Python generation ({} problem selectors)",
        problem_selectors.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let summary = crate::test_runner::runners::run_rslip_selectors(
        repo_root,
        problem_selectors,
        &python_extra,
        false,
        &[],
        jobs,
        None,
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    let deltas = crate::test_runner::python_coverage_index::selector_deltas_from_cached_outcomes(
        repo_root,
        problem_selectors,
        &python_extra,
        &|path, root| {
            python_repo_relative_coverage_file(root, &path.to_string_lossy()).is_some()
        },
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    let _ = crate::test_runner::python_coverage_index::repair_python_population_generation(
        repo_root,
        &deltas,
        crate::test_runner::python_coverage_index::GenerationReason::IncompleteRepair,
    )
    .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
    finalize_incomplete_repair_load(repo_root, &summary)
}

pub(super) fn finalize_incomplete_repair_load(
    repo_root: &Path,
    summary: &SelectorExecutionSummary,
) -> Result<(), CoverageRefreshError> {
    match load_python_runtime_coverage(repo_root) {
        Ok(_) => Ok(()),
        Err(err) if incomplete_repair_became_test_failure(&err, summary.exit_code) => {
            Err(execution_error(summary))
        }
        Err(err) => Err(CoverageRefreshError::validation("Python", err)),
    }
}

pub(super) fn incomplete_repair_became_test_failure(
    err: &RuntimeCoverageLoadError,
    exit_code: i32,
) -> bool {
    err.reason == "incomplete population" && exit_code != 0
}

fn execution_error(summary: &SelectorExecutionSummary) -> CoverageRefreshError {
    CoverageRefreshError::TestExecution {
        language: "Python",
        total: summary.total,
        failed: summary.failed,
        exit_code: summary.exit_code,
    }
}
