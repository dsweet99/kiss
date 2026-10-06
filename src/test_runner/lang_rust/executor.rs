use crate::test_runner::coverage_decision::{LanguageExecutor, RunContext};
use crate::test_runner::lang_rust::backer::RustModule;
use crate::test_runner::runners::{self, SelectorExecutionSummary};

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

        let publication =
            crate::test_runner::rust_coverage_index::rust_selective_rebuild_publication_selectors(
                &ctx.planned.repo_root,
                &ctx.planned.sel.rust,
                ctx.options.extras.rust,
            );
        crate::test_runner::rust_coverage_index::publish_rust_derived_state_with_filter(
            &ctx.planned.repo_root,
            publication,
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

    fn stage_label(&self, population: bool) -> &'static str {
        if population {
            "rust_population"
        } else {
            "rust_final"
        }
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
