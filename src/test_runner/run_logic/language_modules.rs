use super::runners;
use crate::test_runner::coverage_decision::RunContext;
#[cfg(test)]
use crate::test_runner::python_coverage_index::repo_relative_coverage_file as python_repo_relative_coverage_file;
use crate::test_runner::python_coverage_index::{
    rebuild_python_coverage_index, write_python_population_manifest_for_args,
};
use crate::test_runner::runners::SelectorExecutionSummary;
#[cfg(test)]
use crate::test_runner::rust_coverage_index::repo_relative_coverage_file as rust_repo_relative_coverage_file;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args,
};

pub(super) fn python_population_required(ctx: &RunContext<'_, '_>) -> bool {
    ctx.planned.python_population_required
}

pub(super) fn python_population_selectors(ctx: &RunContext<'_, '_>) -> Result<Vec<String>, String> {
    Ok(ctx.planned.python_population_selectors.clone())
}

pub(super) fn python_selective_selectors(ctx: &RunContext<'_, '_>) -> Vec<String> {
    ctx.planned.py_sel.clone()
}

pub(super) fn python_run_population(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    run_rslip_selectors_for_module(selectors, ctx)
}

pub(super) fn python_run_selective(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    run_rslip_selectors_for_module(selectors, ctx)
}

pub(super) fn python_rebuild_index(ctx: &RunContext<'_, '_>) -> Result<(), String> {
    rebuild_python_coverage_index(&ctx.planned.repo_root)?;
    Ok(())
}

pub(super) fn python_write_manifest(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<(), String> {
    write_python_population_manifest_for_args(&ctx.planned.repo_root, selectors, ctx.options.extra)
}

#[cfg(test)]
pub(super) fn python_is_indexable_source(
    path: &std::path::Path,
    repo_root: &std::path::Path,
) -> bool {
    python_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
}

pub(super) fn rust_population_required(ctx: &RunContext<'_, '_>) -> bool {
    !ctx.planned.rust_source_population_paths.is_empty()
}

pub(super) fn rust_population_selectors(ctx: &RunContext<'_, '_>) -> Result<Vec<String>, String> {
    runners::enumerate_workspace_rust_selectors(&ctx.planned.repo_root, &ctx.planned.ignore)
}

pub(super) fn rust_selective_selectors(ctx: &RunContext<'_, '_>) -> Vec<String> {
    ctx.planned.rs_sel.clone()
}

pub(super) fn rust_run_population(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_selectors_for_module(selectors, ctx)
}

pub(super) fn rust_run_selective(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    run_rust_selectors_for_module(selectors, ctx)
}

pub(super) fn rust_rebuild_index(ctx: &RunContext<'_, '_>) -> Result<(), String> {
    rebuild_rust_coverage_index(&ctx.planned.repo_root)?;
    Ok(())
}

pub(super) fn rust_write_manifest(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<(), String> {
    write_rust_population_manifest_for_args(&ctx.planned.repo_root, selectors, ctx.options.extra)
}

#[cfg(test)]
pub(super) fn rust_is_indexable_source(
    path: &std::path::Path,
    repo_root: &std::path::Path,
) -> bool {
    rust_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
}

pub(super) fn run_rslip_selectors_for_module(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let force_rerun =
        ctx.options.force_rerun || !ctx.planned.python_prior_failure_selectors.is_empty();
    runners::run_rslip_selectors(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.extra,
        force_rerun,
        ctx.options.jobs,
    )
}

pub(super) fn run_rust_selectors_for_module(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    let force_rerun =
        ctx.options.force_rerun || !ctx.planned.rust_prior_failure_selectors.is_empty();
    runners::run_rust_llvm_cov_selectors(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.extra,
        force_rerun,
        ctx.options.jobs,
    )
}

#[cfg(test)]
#[path = "language_modules_test.rs"]
mod tests;
