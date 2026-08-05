use super::runners;
use crate::test_runner::coverage_decision::{LanguageExecutor, RunContext};
use crate::test_runner::python_coverage_index::publish_python_derived_state_with_filter;
use crate::test_runner::python_coverage_index::repo_relative_coverage_file as python_repo_relative_coverage_file;
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::runners::rust_backer::RustModule;

impl LanguageExecutor for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        ctx.planned.python_population_required
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        ctx.planned.py_sel.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        run_rslip_selectors_for_module(selectors, ctx)
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        run_rslip_selectors_for_module(selectors, ctx)
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        publish_python_derived_state_with_filter(
            &ctx.planned.repo_root,
            None,
            ctx.options.python_extra,
            |path, repo_root| self.is_indexable_source(path, repo_root),
        )?;
        Ok(())
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        // Manifest currentness alone is not enough: a corrupt index.json must still
        // be republished (QA concurrent-cache-recovery corrupts index on purpose).
        if crate::test_runner::python_coverage_index::python_population_manifest_is_current_for_args_with_env_keys(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.python_extra,
            crate::test_runner::python_coverage_index::PYTHON_COVERAGE_ENV_KEYS,
        ) && crate::test_runner::python_coverage_index::load_current_python_coverage_index(
            &ctx.planned.repo_root,
        )
        .is_some()
        {
            return Ok(());
        }
        publish_python_derived_state_with_filter(
            &ctx.planned.repo_root,
            Some(selectors),
            ctx.options.python_extra,
            |path, repo_root| self.is_indexable_source(path, repo_root),
        )?;
        Ok(())
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        python_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
        _jobs: usize,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        if population {
            lines.push("PYTHON COVERAGE POPULATION".to_string());
        }
        if !selectors.is_empty() {
            let argv = runners::build_pytest_argv(selectors, extra);
            lines.push(runners::shell_quote_line(&argv));
        }
        Ok(lines)
    }
}

impl LanguageExecutor for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        ctx.planned.rust_population_required
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        ctx.planned.rs_sel.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
        let manifest_selectors = self.population_manifest_selectors()?;
        run_rust_selectors_for_module(selectors, ctx, Some(manifest_selectors))
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
        run_rust_selectors_for_module(selectors, ctx, None)
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        crate::test_runner::rust_coverage_index::publish_rust_derived_state_with_filter(
            &ctx.planned.repo_root,
            None,
            ctx.options.extra,
            |path, repo_root| self.is_indexable_source(path, repo_root),
        )
    }

    fn write_manifest(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<(), String> {
        if crate::test_runner::rust_coverage_index::rust_population_manifest_is_current_for_args(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extra,
        ) && crate::test_runner::rust_coverage_index::load_current_rust_population_state(
            &ctx.planned.repo_root,
            None,
            ctx.options.extra,
        )
        .is_some()
        {
            return Ok(());
        }
        crate::test_runner::rust_coverage_index::publish_rust_derived_state_with_filter(
            &ctx.planned.repo_root,
            Some(selectors),
            ctx.options.extra,
            |path, repo_root| self.is_indexable_source(path, repo_root),
        )
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        crate::test_runner::rust_coverage_index::repo_relative_coverage_file(
            repo_root,
            &path.to_string_lossy(),
        )
        .is_some()
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
        jobs: usize,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        if population {
            lines.push("RUST COVERAGE POPULATION".to_string());
        }
        lines.extend(runners::build_rust_coverage_batch_dry_run_lines(
            selectors, extra, jobs,
        )?);
        Ok(lines)
    }
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
        ctx.options.python_extra,
        force_rerun,
        ctx.options.jobs,
    )
}

pub(super) fn run_rust_selectors_for_module(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    population_publication_selectors: Option<Vec<String>>,
) -> Result<SelectorExecutionSummary, String> {
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }
    if let Some(population_selectors) = population_publication_selectors {
        // Population runs publish via SelectorEntries (population.json + reverse_line_index).
        // CheckAggregate refresh remains owned by `kiss cov`, not `kiss test`.
        let force_rerun =
            ctx.options.force_rerun || !ctx.planned.rust_prior_failure_selectors.is_empty();
        return runners::run_rust_llvm_cov_selectors(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extra,
            force_rerun,
            ctx.options.jobs,
            Some(population_selectors),
        );
    }
    let force_rerun =
        ctx.options.force_rerun || !ctx.planned.rust_prior_failure_selectors.is_empty();
    if should_try_cached_rust_check_aggregate(force_rerun, &population_publication_selectors)
        && let Some(summary) = runners::cached_rust_check_aggregate_selectors(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extra,
        )?
    {
        return Ok(summary);
    }
    runners::run_rust_llvm_cov_selectors(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.extra,
        force_rerun,
        ctx.options.jobs,
        population_publication_selectors,
    )
}

fn should_try_cached_rust_check_aggregate(
    force_rerun: bool,
    population_publication_selectors: &Option<Vec<String>>,
) -> bool {
    !force_rerun && population_publication_selectors.is_none()
}

/// Test seam mirroring the population branch of [`run_rust_selectors_for_module`]:
/// always SelectorEntries with publication selectors (never CheckAggregate).
#[cfg(test)]
pub(super) fn run_rust_population_selectors_with_batch_deps<D, E>(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    population_publication_selectors: Vec<String>,
    detect_versions: D,
    execute_batch: E,
) -> Result<SelectorExecutionSummary, String>
where
    D: FnOnce(&std::path::Path) -> Result<crate::test_runner::rust_llvm_cov::RustCoverageToolVersions, String>,
    E: FnOnce(
        &rust_llvm_cov_runner::RustCoverageBatchRequest,
        &crate::test_runner::rust_llvm_cov::RustCoverageToolVersions,
    ) -> Result<rust_llvm_cov_runner::RustCoverageBatchResult, String>,
{
    let force_rerun =
        ctx.options.force_rerun || !ctx.planned.rust_prior_failure_selectors.is_empty();
    crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_selectors_with_deps(
        &ctx.planned.repo_root,
        selectors,
        crate::test_runner::rust_llvm_cov::RustCoverageRunOptions {
            extra: ctx.options.extra,
            force_rerun,
            jobs: ctx.options.jobs,
            population_publication_selectors: Some(population_publication_selectors),
            coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        },
        detect_versions,
        execute_batch,
    )
}

#[cfg(test)]
#[path = "language_modules_test.rs"]
mod tests;
