use super::{PlannedSelectors, SelectorRunOptions, runners};
use crate::test_runner::coverage_decision::{LanguageExecutor, LanguageTestModule, RunContext};
use std::time::Instant;

#[path = "run_logic/language_executor.rs"]
mod language_executor;
#[path = "run_logic/language_modules.rs"]
mod language_modules;
#[path = "run_logic/metrics.rs"]
mod metrics;
#[path = "run_logic/metrics_rust.rs"]
mod metrics_rust;
use language_executor::{
    ExecutionPhase, LanguagePhaseOutcome, execute_language_phase, execution_phase,
    population_selector_count, print_dry_run, selective_selector_count,
};
use metrics::LocalRubricMetrics;

pub(crate) fn run_selectors(
    planned: &PlannedSelectors,
    options: SelectorRunOptions<'_>,
) -> Result<i32, String> {
    let total_started = Instant::now();
    if options.jobs == 0 {
        return Err("error: kiss test: jobs must be greater than zero".to_string());
    }
    if !planned_has_work(planned) {
        return Ok(finish_no_work(planned, &options, total_started));
    }
    let ctx = RunContext {
        planned,
        options: &options,
    };
    let python =
        runners::python_backer::PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    let rust = runners::rust_backer::RustModule::for_execution(&planned.repo_root, &planned.ignore);
    let modules: [(&dyn LanguageTestModule, ExecutionPhase); 2] = [
        (&python, execution_phase(&python, &ctx)?),
        (&rust, execution_phase(&rust, &ctx)?),
    ];
    if options.dry_run {
        return finish_dry_run(planned, &options, total_started, &modules);
    }
    run_selected_phases(planned, &options, total_started, &modules)
}

fn finish_no_work(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
) -> i32 {
    println!("{}", runners::NO_COVERING_TESTS_MSG);
    if options.metrics {
        let mut metrics = LocalRubricMetrics::new(
            planned,
            options,
            0,
            false,
            0,
            planned.rs_sel.len(),
            planned.rust_selection_basis,
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    0
}

fn finish_dry_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
    modules: &[(&dyn LanguageTestModule, ExecutionPhase)],
) -> Result<i32, String> {
    print_dry_run(options, modules)?;
    if options.metrics {
        let python_phase = phase_for_language(modules, kiss::Language::Python);
        let rust_phase = phase_for_language(modules, kiss::Language::Rust);
        let mut metrics = LocalRubricMetrics::new(
            planned,
            options,
            population_selector_count(python_phase),
            matches!(rust_phase, ExecutionPhase::Population(_)),
            population_selector_count(rust_phase),
            selective_selector_count(rust_phase),
            planned.rust_selection_basis,
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    Ok(0)
}

fn run_selected_phases(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
    modules: &[(&dyn LanguageTestModule, ExecutionPhase)],
) -> Result<i32, String> {
    let python_phase = phase_for_language(modules, kiss::Language::Python);
    let rust_phase = phase_for_language(modules, kiss::Language::Rust);
    let mut metrics = LocalRubricMetrics::new(
        planned,
        options,
        population_selector_count(python_phase),
        matches!(rust_phase, ExecutionPhase::Population(_)),
        population_selector_count(rust_phase),
        selective_selector_count(rust_phase),
        planned.rust_selection_basis,
    );
    let ctx = RunContext { planned, options };
    for (module, phase) in modules {
        let outcome = execute_language_phase(*module, phase, &ctx)?;
        record_language_outcome(&mut metrics, LanguageExecutor::language(*module), outcome);
    }
    let mut code = metrics.python.summary.exit_code;
    code = runners::merge_exit_codes(code, rust_exit_code(&metrics, rust_phase));
    Ok(finish_run_metrics(
        metrics,
        code,
        total_started,
        planned,
        options,
    ))
}

fn record_language_outcome(
    metrics: &mut LocalRubricMetrics,
    language: kiss::Language,
    outcome: LanguagePhaseOutcome,
) {
    match language {
        kiss::Language::Python => record_python_outcome(metrics, outcome),
        kiss::Language::Rust => record_rust_outcome(metrics, outcome),
    }
}

fn record_python_outcome(metrics: &mut LocalRubricMetrics, outcome: LanguagePhaseOutcome) {
    metrics.python.summary = outcome.summary;
    metrics.python.duration = outcome.phase_duration;
    metrics.python_index_rebuild_duration += outcome.index_rebuild_duration;
}

fn record_rust_outcome(metrics: &mut LocalRubricMetrics, outcome: LanguagePhaseOutcome) {
    match outcome.phase {
        ExecutionPhase::Population(_) => {
            metrics.rust_population.summary = outcome.summary;
            metrics.rust_population.duration = outcome.phase_duration;
            metrics.rust_index_rebuild_duration += outcome.index_rebuild_duration;
        }
        ExecutionPhase::Selective(_) => {
            metrics.rust_final.summary = outcome.summary;
            metrics.rust_final.duration = outcome.phase_duration;
            metrics.rust_index_rebuild_duration += outcome.index_rebuild_duration;
        }
        ExecutionPhase::NoWork => {}
    }
}

fn rust_exit_code(metrics: &LocalRubricMetrics, phase: &ExecutionPhase) -> i32 {
    match phase {
        ExecutionPhase::Population(_) => metrics.rust_population.summary.exit_code,
        ExecutionPhase::Selective(_) => metrics.rust_final.summary.exit_code,
        ExecutionPhase::NoWork => 0,
    }
}

fn phase_for_language<'a>(
    modules: &'a [(&dyn LanguageTestModule, ExecutionPhase)],
    language: kiss::Language,
) -> &'a ExecutionPhase {
    modules
        .iter()
        .find_map(|(module, phase)| {
            (LanguageExecutor::language(*module) == language).then_some(phase)
        })
        .expect("run logic constructs a phase for each supported language")
}

fn planned_has_work(planned: &PlannedSelectors) -> bool {
    planned.python_population_required
        || planned.rust_population_required
        || !planned.py_sel.is_empty()
        || !planned.rs_sel.is_empty()
}

fn finish_run_metrics(
    mut metrics: LocalRubricMetrics,
    code: i32,
    total_started: Instant,
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> i32 {
    metrics.exit_code = code;
    metrics.total_duration = total_started.elapsed();
    metrics.capture_cache_shape(&planned.repo_root);
    if options.metrics {
        metrics.print();
    }
    code
}

#[cfg(test)]
#[path = "run_logic_test.rs"]
mod tests;
