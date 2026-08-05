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

pub(super) struct LanguagePhaseOutcome {
    pub(super) phase: ExecutionPhase,
    pub(super) summary: SelectorExecutionSummary,
    pub(super) phase_duration: std::time::Duration,
    pub(super) index_rebuild_duration: std::time::Duration,
}

pub(super) fn execution_phase(
    module: &dyn LanguageTestModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    if module.population_required(ctx) {
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
        return Ok(ExecutionPhase::Population(selectors));
    }
    let selectors = module.selective_selectors(ctx);
    if selectors.is_empty() {
        Ok(ExecutionPhase::NoWork)
    } else {
        Ok(ExecutionPhase::Selective(selectors))
    }
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
    if matches!(phase, ExecutionPhase::Population(_))
        && summary.exit_code == 0
        && let ExecutionPhase::Population(selectors) = phase
    {
        let index_started = Instant::now();
        module.write_manifest(selectors, ctx)?;
        index_rebuild_duration = index_started.elapsed();
    } else if should_rebuild_index(phase, &summary) {
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

fn should_rebuild_index(phase: &ExecutionPhase, summary: &SelectorExecutionSummary) -> bool {
    match phase {
        ExecutionPhase::NoWork => false,
        ExecutionPhase::Population(_) => summary.exit_code == 0,
        // Cache hits do not change coverage maps; rebuild only after fresh runs.
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
        let extra = match LanguageExecutor::language(*module) {
            kiss::Language::Python => options.python_extra,
            kiss::Language::Rust => options.extra,
        };
        for line in module.dry_run_lines(selectors, population, extra, options.jobs)? {
            println!("{line}");
        }
    }
    Ok(())
}
