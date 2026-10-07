use crate::test_runner::lang_python::backer::PythonModule;
use crate::test_runner::runners::{self, SelectorExecutionSummary};
use crate::test_runner::test_selection::{LanguageExecutor, RunContext};

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
        let _ = (self, ctx);
        Ok(())
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        let _ = (self, selectors, ctx);
        Ok(())
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
            lines.push("PYTHON POPULATION".to_string());
        }
        if !selectors.is_empty() {
            let argv = runners::build_pytest_argv(selectors, extra);
            lines.push(runners::shell_quote_line(&argv));
        }
        Ok(lines)
    }

    fn stage_label(&self, _population: bool) -> &'static str {
        "python"
    }

    fn population_from_plan(&self) -> bool {
        true
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
    assert!(ctx.options.jobs > 0, "jobs must be greater than zero");
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
