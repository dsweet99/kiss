use crate::test_runner::lang_rust::backer::RustModule;
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::test_selection::{LanguageExecutor, RunContext};

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
        crate::test_runner::ensure_runtime::ensure_language_via_kernel(
            kiss::Language::Rust,
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
        crate::test_runner::ensure_runtime::ensure_language_via_kernel(
            kiss::Language::Rust,
            selectors,
            ctx,
            crate::test_runner::lang_iface::AcceptMode::Subset,
        )
    }

    fn rebuild_index(&self, _ctx: &RunContext<'_, '_>) -> Result<(), String> {
        Ok(())
    }

    fn write_manifest(
        &self,
        _selectors: &[String],
        _ctx: &RunContext<'_, '_>,
    ) -> Result<(), String> {
        Ok(())
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
            lines.push("RUST POPULATION".to_string());
        }
        lines.extend(super::nextest::dry_run_lines(selectors, extra, jobs)?);
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
