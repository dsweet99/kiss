use std::time::Instant;

use crate::test_runner::coverage_decision::{
    LanguageExecutor, LanguagePlanner, LanguageTestModule, RunContext,
};
use crate::test_runner::runners::SelectorExecutionSummary;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ExecutionPhase {
    NoWork,
    Population(Vec<String>),
    Selective(Vec<String>),
}

pub(crate) struct LanguagePhaseOutcome {
    pub(super) phase: ExecutionPhase,
    pub(super) summary: SelectorExecutionSummary,
    pub(super) phase_duration: std::time::Duration,
    pub(super) index_rebuild_duration: std::time::Duration,
}

#[cfg(test)]
impl LanguagePhaseOutcome {
    pub(crate) fn test_selective(exit_code: i32) -> Self {
        Self {
            phase: ExecutionPhase::Selective(vec!["sel".into()]),
            summary: SelectorExecutionSummary {
                exit_code,
                total: 1,
                failed: usize::from(exit_code != 0),
                ..SelectorExecutionSummary::default()
            },
            phase_duration: std::time::Duration::ZERO,
            index_rebuild_duration: std::time::Duration::ZERO,
        }
    }
}

pub(super) fn execution_phase(
    module: &dyn LanguageTestModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    if module.population_required(ctx) {
        let language = LanguagePlanner::language(module);
        if language == kiss::Language::Rust {
            return rust_population_phase(module, ctx);
        }
        return discover_population_selectors(module, ctx);
    }
    let selectors = module.selective_selectors(ctx);
    if selectors.is_empty() {
        Ok(ExecutionPhase::NoWork)
    } else {
        Ok(ExecutionPhase::Selective(selectors))
    }
}

fn rust_population_phase(
    module: &dyn LanguageTestModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    if crate::test_runner::rust_list_build::covering_population_list_build_done() {
        return planned_population_selectors(module, ctx);
    }
    crate::test_runner::rust_list_build::overlap_with_discover(|| {
        discover_population_selectors(module, ctx)
    })
}

fn planned_population_selectors(
    module: &dyn LanguageTestModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    let language = LanguagePlanner::language(module);
    let mut selectors = match language {
        kiss::Language::Python => ctx.planned.sel.python.clone(),
        kiss::Language::Rust => ctx.planned.sel.rust.clone(),
    };
    selectors.extend(module.selective_selectors(ctx));
    selectors.sort();
    selectors.dedup();
    Ok(ExecutionPhase::Population(selectors))
}

fn discover_population_selectors(
    module: &dyn LanguageTestModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    let language = LanguagePlanner::language(module);
    let mut selectors: Vec<_> = LanguagePlanner::discover_universe(module)?
        .into_iter()
        .map(|selector| {
            assert_eq!(
                selector.language, language,
                "discover_universe must return only selectors for the module language"
            );
            selector.id
        })
        .collect();
    selectors.extend(module.selective_selectors(ctx));
    selectors.sort();
    selectors.dedup();
    Ok(ExecutionPhase::Population(selectors))
}

pub(super) fn execute_language_phase(
    module: &dyn LanguageTestModule,
    phase: &ExecutionPhase,
    ctx: &RunContext<'_, '_>,
) -> Result<LanguagePhaseOutcome, String> {
    let started = Instant::now();
    let summary = match phase {
        ExecutionPhase::NoWork => SelectorExecutionSummary::default(),
        ExecutionPhase::Population(selectors) => module.run_population(selectors, ctx)?,
        ExecutionPhase::Selective(selectors) => module.run_selective(selectors, ctx)?,
    };
    let phase_duration = started.elapsed();
    let mut index_rebuild_duration = std::time::Duration::ZERO;

    if let ExecutionPhase::Population(selectors) = phase {
        let index_started = Instant::now();
        module.write_manifest(selectors, ctx)?;
        index_rebuild_duration = index_started.elapsed();
    } else if should_rebuild_after_selective(module, phase, &summary) {
        let index_started = Instant::now();
        module.rebuild_index(ctx)?;
        index_rebuild_duration = index_started.elapsed();
    }
    Ok(LanguagePhaseOutcome {
        phase: phase.clone(),
        summary,
        phase_duration,
        index_rebuild_duration,
    })
}

fn should_rebuild_after_selective(
    _module: &dyn LanguageTestModule,
    phase: &ExecutionPhase,
    summary: &SelectorExecutionSummary,
) -> bool {
    match phase {
        ExecutionPhase::NoWork | ExecutionPhase::Population(_) => false,
        ExecutionPhase::Selective(_) => summary.cache_misses > 0 || summary.cache_unstored > 0,
    }
}

pub(super) fn population_selector_count(phase: &ExecutionPhase) -> usize {
    match phase {
        ExecutionPhase::Population(selectors) => selectors.len(),
        ExecutionPhase::Selective(_) | ExecutionPhase::NoWork => 0,
    }
}

pub(super) fn selective_selector_count(phase: &ExecutionPhase) -> usize {
    match phase {
        ExecutionPhase::Selective(selectors) => selectors.len(),
        ExecutionPhase::Population(_) | ExecutionPhase::NoWork => 0,
    }
}

pub(super) fn print_dry_run(
    options: &crate::test_runner::SelectorRunOptions<'_>,
    phases: &[(&dyn LanguageTestModule, ExecutionPhase)],
) -> Result<(), String> {
    for (module, phase) in phases {
        let (selectors, population) = match phase {
            ExecutionPhase::NoWork => continue,
            ExecutionPhase::Population(selectors) => (selectors.as_slice(), true),
            ExecutionPhase::Selective(selectors) => (selectors.as_slice(), false),
        };
        let extra = *options.extras.get(LanguageExecutor::language(*module));
        for line in module.dry_run_lines(selectors, population, extra, options.jobs)? {
            crate::test_runner::emit_test_progress(&line);
        }
    }
    Ok(())
}
