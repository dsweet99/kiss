//! Python incomplete-generation repair and full cold refresh for `kiss test`.

use std::path::Path;

use kiss::GateConfig;

use super::{
    CoverageRefreshError, CoverageRefreshStats, LanguageRefreshStats, ScopedRefreshEnvGuard,
    lock_refresh,
};
use crate::test_runner::check_line_coverage::{
    RuntimeCoverageLoadError, load_python_runtime_coverage,
};
use crate::test_runner::ensure_runtime::{
    ensure_languages_runtime, ensure_request_for_all, ensure_request_for_selectors,
};
use crate::test_runner::python_coverage_index::try_load_pinned_python_generation;
use crate::test_runner::runners::SelectorExecutionSummary;

pub(super) fn ensure_python_runtime_coverage(
    repo_root: &Path,
    ignore: &[String],
    jobs: usize,
    pytest_args: &[String],
    gate: &GateConfig,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    let _guard = lock_refresh(repo_root, "Python")?;
    match load_python_runtime_coverage(repo_root, pytest_args, gate) {
        Ok(_) => return Ok(CoverageRefreshStats::default()),
        Err(err) if !err.problem_selectors.is_empty() => {
            let planned = planned_selectors_for_incomplete(repo_root, &err.problem_selectors);
            return run_python_ensure(
                ensure_request_for_selectors(crate::test_runner::ensure_runtime::EnsureSelectorsArgs {
                    repo_root,
                    ignore,
                    jobs,
                    lang_filter: kiss::Language::Python,
                    force: false,
                    python: planned,
                    rust: vec![],
                    gate: gate.clone(),
                    pytest_args: pytest_args.to_vec(),
                }),
                pytest_args,
                gate,
            );
        }
        Err(_) => {}
    }
    let request = ensure_request_for_all(
        repo_root,
        ignore,
        jobs,
        Some(kiss::Language::Python),
        false,
        gate.clone(),
        pytest_args.to_vec(),
    )
    .map_err(|err| CoverageRefreshError::discovery("Python", err))?;
    run_python_ensure(request, pytest_args, gate)
}

fn planned_selectors_for_incomplete(
    repo_root: &Path,
    problem_selectors: &[String],
) -> Vec<String> {
    if let Ok(pinned) = try_load_pinned_python_generation(repo_root) {
        let mut planned = pinned.plan.selectors;
        planned.sort();
        planned.dedup();
        if !planned.is_empty() {
            return planned;
        }
    }
    problem_selectors.to_vec()
}

fn run_python_ensure(
    request: crate::test_runner::lang_iface::EnsureRequest,
    pytest_args: &[String],
    gate: &GateConfig,
) -> Result<CoverageRefreshStats, CoverageRefreshError> {
    eprintln!(
        "kiss test: refreshing Python runtime coverage ({} tests)",
        request.planned.python.len()
    );
    let _refresh_env = ScopedRefreshEnvGuard::set();
    let result = ensure_languages_runtime(&request)
        .map_err(|err| CoverageRefreshError::publication("Python", err))?;
    let summary = result
        .python()
        .map(|r| r.summary.clone())
        .unwrap_or_default();
    if summary.exit_code != 0 {
        return Err(execution_error(&summary));
    }
    finalize_incomplete_repair_load(&request.repo_root, &summary, pytest_args, gate)?;
    Ok(CoverageRefreshStats {
        by_language: crate::test_runner::language_keyed::LanguageKeyed {
            python: LanguageRefreshStats {
                test_instances: summary.total,
                full_refresh: true,
                ..Default::default()
            },
            rust: LanguageRefreshStats::default(),
        },
    })
}

pub(super) fn finalize_incomplete_repair_load(
    repo_root: &Path,
    summary: &SelectorExecutionSummary,
    pytest_args: &[String],
    gate: &GateConfig,
) -> Result<(), CoverageRefreshError> {
    match load_python_runtime_coverage(repo_root, pytest_args, gate) {
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
