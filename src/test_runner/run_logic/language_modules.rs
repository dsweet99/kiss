use super::runners;
use crate::test_runner::coverage_decision::{LanguageExecutor, RunContext};
use crate::test_runner::python_coverage_index::repo_relative_coverage_file as python_repo_relative_coverage_file;
use crate::test_runner::python_coverage_index::{
    rebuild_python_coverage_index_with_filter, write_python_population_manifest_for_args,
};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::runners::python_backer::PythonModule;
use crate::test_runner::runners::rust_backer::RustModule;
use crate::test_runner::rust_coverage_index::repo_relative_coverage_file as rust_repo_relative_coverage_file;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index_with_filter, write_rust_population_manifest_for_args,
};

impl LanguageExecutor for PythonModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Python
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        ctx.planned.python_population_required
    }

    fn population_selectors(&self, ctx: &RunContext<'_, '_>) -> Result<Vec<String>, String> {
        Ok(ctx.planned.python_population_selectors.clone())
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
        rebuild_python_coverage_index_with_filter(&ctx.planned.repo_root, |path, repo_root| {
            self.is_indexable_source(path, repo_root)
        })?;
        Ok(())
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        write_python_population_manifest_for_args(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extra,
        )
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        python_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if population {
            lines.push("PYTHON COVERAGE POPULATION".to_string());
        }
        if !selectors.is_empty() {
            let argv = runners::build_pytest_argv(selectors, extra);
            lines.push(runners::shell_quote_line(&argv));
        }
        lines
    }
}

impl LanguageExecutor for RustModule {
    fn language(&self) -> kiss::Language {
        kiss::Language::Rust
    }

    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        !ctx.planned.rust_source_population_paths.is_empty()
    }

    fn population_selectors(&self, ctx: &RunContext<'_, '_>) -> Result<Vec<String>, String> {
        Ok(ctx.planned.rust_population_selectors.clone())
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        ctx.planned.rs_sel.clone()
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        run_rust_selectors_for_module(selectors, ctx)
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        run_rust_selectors_for_module(selectors, ctx)
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        rebuild_rust_coverage_index_with_filter(&ctx.planned.repo_root, |path, repo_root| {
            self.is_indexable_source(path, repo_root)
        })?;
        Ok(())
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        write_rust_population_manifest_for_args(
            &ctx.planned.repo_root,
            selectors,
            ctx.options.extra,
        )
    }

    fn is_indexable_source(&self, path: &std::path::Path, repo_root: &std::path::Path) -> bool {
        rust_repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if population {
            lines.push("RUST COVERAGE POPULATION".to_string());
        }
        for selector in selectors {
            let argv = runners::build_cargo_llvm_cov_dry_run_argv(selector, extra);
            lines.push(runners::shell_quote_line(&argv));
        }
        lines
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
