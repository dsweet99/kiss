use std::time::Instant;

use super::language_modules;
use super::runners;
use crate::test_runner::coverage_decision::RunContext;
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

pub(super) enum ExecutionModule {
    Python,
    Rust,
}

impl ExecutionModule {
    fn population_required(&self, ctx: &RunContext<'_, '_>) -> bool {
        match self {
            Self::Python => language_modules::python_population_required(ctx),
            Self::Rust => language_modules::rust_population_required(ctx),
        }
    }

    fn population_selectors(&self, ctx: &RunContext<'_, '_>) -> Result<Vec<String>, String> {
        match self {
            Self::Python => language_modules::python_population_selectors(ctx),
            Self::Rust => language_modules::rust_population_selectors(ctx),
        }
    }

    fn selective_selectors(&self, ctx: &RunContext<'_, '_>) -> Vec<String> {
        match self {
            Self::Python => language_modules::python_selective_selectors(ctx),
            Self::Rust => language_modules::rust_selective_selectors(ctx),
        }
    }

    fn run_population(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        match self {
            Self::Python => language_modules::python_run_population(selectors, ctx),
            Self::Rust => language_modules::rust_run_population(selectors, ctx),
        }
    }

    fn run_selective(
        &self,
        selectors: &[String],
        ctx: &RunContext<'_, '_>,
    ) -> Result<SelectorExecutionSummary, String> {
        match self {
            Self::Python => language_modules::python_run_selective(selectors, ctx),
            Self::Rust => language_modules::rust_run_selective(selectors, ctx),
        }
    }

    fn rebuild_index(&self, ctx: &RunContext<'_, '_>) -> Result<(), String> {
        match self {
            Self::Python => language_modules::python_rebuild_index(ctx),
            Self::Rust => language_modules::rust_rebuild_index(ctx),
        }
    }

    fn write_manifest(&self, selectors: &[String], ctx: &RunContext<'_, '_>) -> Result<(), String> {
        match self {
            Self::Python => language_modules::python_write_manifest(selectors, ctx),
            Self::Rust => language_modules::rust_write_manifest(selectors, ctx),
        }
    }
}

pub(super) fn execution_phase(
    module: &ExecutionModule,
    ctx: &RunContext<'_, '_>,
) -> Result<ExecutionPhase, String> {
    if module.population_required(ctx) {
        return Ok(ExecutionPhase::Population(
            module.population_selectors(ctx)?,
        ));
    }
    let selectors = module.selective_selectors(ctx);
    if selectors.is_empty() {
        Ok(ExecutionPhase::NoWork)
    } else {
        Ok(ExecutionPhase::Selective(selectors))
    }
}

pub(super) fn execute_language_phase(
    module: &ExecutionModule,
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
    if should_rebuild_index(phase, &summary) {
        let index_started = Instant::now();
        module.rebuild_index(ctx)?;
        index_rebuild_duration = index_started.elapsed();
    }
    if matches!(phase, ExecutionPhase::Population(_))
        && summary.exit_code == 0
        && let ExecutionPhase::Population(selectors) = phase
    {
        module.write_manifest(selectors, ctx)?;
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
        ExecutionPhase::Selective(_) => summary.total > 0,
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
    python_phase: &ExecutionPhase,
    rust_phase: &ExecutionPhase,
) {
    match python_phase {
        ExecutionPhase::Population(selectors) => {
            println!("PYTHON COVERAGE POPULATION");
            if !selectors.is_empty() {
                let argv = runners::build_pytest_argv(selectors, options.extra);
                println!("{}", runners::shell_quote_line(&argv));
            }
        }
        ExecutionPhase::Selective(selectors) => {
            let argv = runners::build_pytest_argv(selectors, options.extra);
            println!("{}", runners::shell_quote_line(&argv));
        }
        ExecutionPhase::NoWork => {}
    }
    match rust_phase {
        ExecutionPhase::Population(selectors) => {
            println!("RUST COVERAGE POPULATION");
            print_rust_dry_run_selectors(selectors, options);
        }
        ExecutionPhase::Selective(selectors) => print_rust_dry_run_selectors(selectors, options),
        ExecutionPhase::NoWork => {}
    }
}

fn print_rust_dry_run_selectors(
    selectors: &[String],
    options: &crate::test_runner::SelectorRunOptions<'_>,
) {
    for selector in selectors {
        let argv = runners::build_cargo_llvm_cov_dry_run_argv(selector, options.extra);
        println!("{}", runners::shell_quote_line(&argv));
    }
}
