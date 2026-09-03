use super::runners;
use crate::test_runner::coverage_decision::{LanguageExecutor, RunContext};
use crate::test_runner::execution_witness::{
    RustWarmDecision, maybe_bootstrap_rust_witness, rust_warm_or_miss_selectors,
};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::runners::rust_backer::RustModule;

#[path = "python_generation_hooks.rs"]
mod python_generation_hooks;

impl LanguageExecutor for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        ctx.planned.population_required.python
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        ctx.planned.sel.python.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        ensure_python_via_kernel(
            selectors,
            ctx,
            crate::test_runner::lang_iface::AcceptMode::All,
        )
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        ensure_python_via_kernel(
            selectors,
            ctx,
            crate::test_runner::lang_iface::AcceptMode::Subset,
        )
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        let index = crate::test_runner::coverage_index::for_language(kiss::Language::Python);
        let _ = (
            index.cache_root(&ctx.planned.repo_root),
            index.index_file_present(&ctx.planned.repo_root),
        );
        python_generation_hooks::rebuild_python_index(self, ctx)
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        let _ = (self, selectors, ctx);
        Ok(())
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        crate::test_runner::coverage_index::for_language(kiss::Language::Python)
            .repo_relative_coverage_file(repo_root, &path.to_string_lossy())
            .is_some()
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

fn ensure_python_via_kernel(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    mode: crate::test_runner::lang_iface::AcceptMode,
) -> Result<SelectorExecutionSummary, String> {
    use crate::test_runner::ensure_runtime::{
        ensure_languages_runtime, ensure_request_from_planned,
    };
    let mut planned = ctx.planned.clone();
    planned.sel.python = selectors.to_vec();
    planned.sel.rust.clear();
    let request =
        ensure_request_from_planned(crate::test_runner::ensure_runtime::EnsureFromPlanned {
            planned: &planned,
            mode,
            lang_filter: Some(kiss::Language::Python),
            force: ctx.options.force_rerun,
            force_selectors: ctx.planned.prior_failure_selectors.python.clone(),
            jobs: ctx.options.jobs,
            extras: ctx.options.extras,
            repo_root_override: None,
            gate: ctx.options.gate.clone(),
        });
    let result = ensure_languages_runtime(&request)?;
    Ok(result
        .python()
        .map(|r| r.summary.clone())
        .unwrap_or_default())
}

impl LanguageExecutor for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        ctx.planned.population_required.rust
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        ctx.planned.sel.rust.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
        ensure_rust_via_kernel(
            selectors,
            ctx,
            crate::test_runner::lang_iface::AcceptMode::All,
        )
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
        ensure_rust_via_kernel(
            selectors,
            ctx,
            crate::test_runner::lang_iface::AcceptMode::Subset,
        )
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        let index = crate::test_runner::coverage_index::for_language(kiss::Language::Rust);
        let _ = (
            index.cache_root(&ctx.planned.repo_root),
            index.index_file_present(&ctx.planned.repo_root),
        );

        crate::test_runner::rust_coverage_index::publish_rust_derived_state_with_filter(
            &ctx.planned.repo_root,
            None,
            ctx.options.extras.rust,
            |path, repo_root| self.is_indexable_source(path, repo_root),
        )
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        let _ = (self, selectors, ctx);
        Ok(())
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        crate::test_runner::coverage_index::for_language(kiss::Language::Rust)
            .repo_relative_coverage_file(repo_root, &path.to_string_lossy())
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

fn ensure_rust_via_kernel(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    mode: crate::test_runner::lang_iface::AcceptMode,
) -> Result<SelectorExecutionSummary, String> {
    use crate::test_runner::ensure_runtime::{
        ensure_languages_runtime, ensure_request_from_planned,
    };

    let force = ctx.options.force_rerun;
    let mut planned = ctx.planned.clone();
    planned.sel.rust = selectors.to_vec();
    planned.sel.python.clear();
    let request =
        ensure_request_from_planned(crate::test_runner::ensure_runtime::EnsureFromPlanned {
            planned: &planned,
            mode,
            lang_filter: Some(kiss::Language::Rust),
            force,
            force_selectors: ctx.planned.prior_failure_selectors.rust.clone(),
            jobs: ctx.options.jobs,
            extras: ctx.options.extras,
            repo_root_override: None,
            gate: ctx.options.gate.clone(),
        });
    let result = ensure_languages_runtime(&request)?;
    Ok(result.rust().map(|r| r.summary.clone()).unwrap_or_default())
}

#[allow(dead_code)]
pub(super) fn run_rslip_selectors_for_module(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<SelectorExecutionSummary, String> {
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }

    runners::run_rslip_selectors(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.extras.python,
        ctx.options.force_rerun,
        &ctx.planned.prior_failure_selectors.python,
        ctx.options.jobs,
        ctx.planned.workspace_files_fingerprint.clone(),
        &ctx.options.gate,
    )
}

#[allow(dead_code)]
pub(super) fn run_rust_selectors_for_module(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    population_publication_selectors: Option<Vec<String>>,
) -> Result<SelectorExecutionSummary, String> {
    if selectors.is_empty() {
        return Ok(SelectorExecutionSummary::default());
    }

    let force_rerun = ctx.options.force_rerun;
    if !force_rerun
        && let Ok(identity) =
            crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
                &ctx.planned.repo_root,
                ctx.options.extras.rust,
            )
    {
        maybe_bootstrap_rust_witness(&ctx.planned.repo_root, selectors, &identity);
        let warm = rust_warm_or_miss_selectors(
            &ctx.planned.repo_root,
            selectors,
            &identity,
            &ctx.options.gate,
        );
        let mut run_only: Option<Vec<String>> = match warm {
            RustWarmDecision::Warm(summary) => {
                let forced: Vec<String> = ctx
                    .planned
                    .prior_failure_selectors
                    .rust
                    .iter()
                    .filter(|sel| selectors.iter().any(|s| s == *sel))
                    .cloned()
                    .collect();
                if forced.is_empty() {
                    return Ok(*summary);
                }
                Some(forced)
            }
            RustWarmDecision::RunMisses(misses) => Some(misses),
            RustWarmDecision::Miss => None,
        };
        if let Some(ref mut misses) = run_only {
            crate::test_runner::lang_iface::union_force_selectors_into_misses(
                selectors,
                misses,
                &ctx.planned.prior_failure_selectors.rust,
            );
            return runners::run_rust_llvm_cov_selectors(
                &ctx.planned.repo_root,
                misses,
                ctx.options.extras.rust,
                force_rerun,
                &ctx.planned.prior_failure_selectors.rust,
                ctx.options.jobs,
                population_publication_selectors.clone(),
                &ctx.options.gate,
            );
        }
    }
    if let Some(population_selectors) = population_publication_selectors {
        return runners::run_rust_llvm_cov_selectors(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extras.rust,
            force_rerun,
            &ctx.planned.prior_failure_selectors.rust,
            ctx.options.jobs,
            Some(population_selectors),
            &ctx.options.gate,
        );
    }
    if should_try_cached_rust_check_aggregate(force_rerun, &None)
        && let Some(summary) = runners::cached_rust_check_aggregate_selectors(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extras.rust,
        )?
    {
        return Ok(summary);
    }
    runners::run_rust_llvm_cov_selectors(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.extras.rust,
        force_rerun,
        &ctx.planned.prior_failure_selectors.rust,
        ctx.options.jobs,
        None,
        &ctx.options.gate,
    )
}

fn should_try_cached_rust_check_aggregate(
    force_rerun: bool,
    population_publication_selectors: &Option<Vec<String>>,
) -> bool {
    !force_rerun && population_publication_selectors.is_none()
}

#[cfg(test)]
pub(super) fn run_rust_population_selectors_with_batch_deps<D, E>(
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
    population_publication_selectors: Vec<String>,
    detect_versions: D,
    execute_batch: E,
) -> Result<SelectorExecutionSummary, String>
where
    D: FnOnce(
        &std::path::Path,
    ) -> Result<crate::test_runner::rust_llvm_cov::RustCoverageToolVersions, String>,
    E: FnOnce(
        &kiss::rust_llvm_cov_runner::RustCoverageBatchRequest,
        &crate::test_runner::rust_llvm_cov::RustCoverageToolVersions,
    ) -> Result<kiss::rust_llvm_cov_runner::RustCoverageBatchResult, String>,
{
    let force_rerun = ctx.options.force_rerun;
    crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_selectors_with_deps(
        &ctx.planned.repo_root,
        selectors,
        crate::test_runner::rust_llvm_cov::RustCoverageRunOptions {
            extra: ctx.options.extras.rust,
            force_rerun,
            force_rerun_selectors: &ctx.planned.prior_failure_selectors.rust,
            jobs: ctx.options.jobs,
            population_publication_selectors: Some(population_publication_selectors),
            coverage_output_mode: kiss::rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
            gate: kiss::GateConfig::default(),
        },
        detect_versions,
        execute_batch,
    )
}

#[cfg(test)]
fn dry_run_selector_options() -> crate::test_runner::SelectorRunOptions<'static> {
    crate::test_runner::SelectorRunOptions {
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        plan_duration: std::time::Duration::ZERO,
        gate: kiss::GateConfig::default(),
    }
}

#[cfg(test)]
#[path = "language_modules_test.rs"]
mod tests;

#[cfg(test)]
#[path = "language_modules_force_test.rs"]
mod force_tests;
