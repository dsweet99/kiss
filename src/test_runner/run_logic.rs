use super::{PlannedSelectors, SelectorRunOptions, runners};
use crate::test_runner::coverage_decision::RunContext;
use std::time::Instant;

#[path = "run_logic/language_executor.rs"]
mod language_executor;
#[path = "run_logic/language_modules.rs"]
mod language_modules;
#[path = "run_logic/metrics.rs"]
mod metrics;
use language_executor::{
    ExecutionModule, ExecutionPhase, LanguagePhaseOutcome, execute_language_phase, execution_phase,
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
    let python = ExecutionModule::Python;
    let rust = ExecutionModule::Rust;
    let python_phase = execution_phase(&python, &ctx)?;
    let rust_phase = execution_phase(&rust, &ctx)?;
    if options.dry_run {
        return Ok(finish_dry_run(
            planned,
            &options,
            total_started,
            &python_phase,
            &rust_phase,
        ));
    }
    run_selected_phases(planned, &options, total_started, &python_phase, &rust_phase)
}

fn finish_no_work(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
) -> i32 {
    println!("{}", runners::NO_COVERING_TESTS_MSG);
    if options.metrics {
        let mut metrics = LocalRubricMetrics::new(planned, options, false, 0, planned.rs_sel.len());
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
    python_phase: &ExecutionPhase,
    rust_phase: &ExecutionPhase,
) -> i32 {
    print_dry_run(options, python_phase, rust_phase);
    if options.metrics {
        let rust_population_selectors = population_selector_count(rust_phase);
        let rust_final_selectors = selective_selector_count(rust_phase);
        let mut metrics = LocalRubricMetrics::new(
            planned,
            options,
            matches!(rust_phase, ExecutionPhase::Population(_)),
            rust_population_selectors,
            rust_final_selectors,
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    0
}

fn run_selected_phases(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    total_started: Instant,
    python_phase: &ExecutionPhase,
    rust_phase: &ExecutionPhase,
) -> Result<i32, String> {
    let rust_population_selectors = population_selector_count(rust_phase);
    let rust_final_selectors = selective_selector_count(rust_phase);
    let mut metrics = LocalRubricMetrics::new(
        planned,
        options,
        matches!(rust_phase, ExecutionPhase::Population(_)),
        rust_population_selectors,
        rust_final_selectors,
    );
    let ctx = RunContext { planned, options };
    let python = ExecutionModule::Python;
    let python_outcome = execute_language_phase(&python, python_phase, &ctx)?;
    record_python_outcome(&mut metrics, python_outcome);
    let mut code = metrics.python.summary.exit_code;
    let rust = ExecutionModule::Rust;
    let rust_outcome = execute_language_phase(&rust, rust_phase, &ctx)?;
    record_rust_outcome(&mut metrics, rust_outcome);
    code = runners::merge_exit_codes(code, rust_exit_code(&metrics, rust_phase));
    Ok(finish_run_metrics(
        metrics,
        code,
        total_started,
        planned,
        options,
    ))
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

fn planned_has_work(planned: &PlannedSelectors) -> bool {
    planned.python_population_required
        || !planned.py_sel.is_empty()
        || !planned.rs_sel.is_empty()
        || !planned.rust_source_population_paths.is_empty()
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
