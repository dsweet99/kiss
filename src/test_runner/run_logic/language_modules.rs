use crate::test_runner::coverage_decision::{LanguageExecutor, RunContext};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::runners::rust_backer::RustModule;

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

#[path = "language_modules_test.rs"]
mod tests;

#[path = "language_modules_force_test.rs"]
mod force_tests;
