use super::{PlannedSelectors, SelectorRunOptions, runners};
use crate::test_runner::coverage_decision::{LanguageExecutor, LanguageTestModule, RunContext};
use crate::test_runner::final_summary::{FinalTestSummary, print_final_test_summary};
use std::time::{Duration, Instant};

#[path = "run_logic/cache_decision_metrics.rs"]
mod cache_decision_metrics;
#[path = "run_logic/language_executor.rs"]
mod language_executor;
#[path = "run_logic/language_modules.rs"]
mod language_modules;
#[path = "run_logic/metrics.rs"]
mod metrics;
#[path = "run_logic/metrics_rust.rs"]
mod metrics_rust;
pub(crate) use language_executor::LanguagePhaseOutcome;
use language_executor::{
    ExecutionPhase, execute_language_phase, execution_phase, population_selector_count,
    print_dry_run, selective_selector_count,
};
use metrics::LocalRubricMetrics;

#[cfg_attr(not(test), allow(dead_code))]
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
    let python = runners::python_backer::PythonModule::for_execution_with_args(
        &planned.repo_root,
        &planned.ignore,
        options.extras.python,
    );
    let rust = runners::rust_backer::RustModule::for_execution(&planned.repo_root, &planned.ignore);
    let python_phase = execution_phase(&python, &ctx)?;
    let rust_phase = execution_phase(&rust, &ctx)?;
    let modules: [(&dyn LanguageTestModule, ExecutionPhase); 2] =
        [(&python, python_phase), (&rust, rust_phase)];
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
    crate::test_runner::emit_test_progress(runners::NO_COVERING_TESTS_MSG);
    if options.metrics {
        let mut metrics = LocalRubricMetrics::new(
            planned,
            options,
            0,
            false,
            0,
            planned.sel.rust.len(),
            planned.selection_basis.rust,
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    print_final_test_summary(
        &FinalTestSummary::default(),
        summary_total_duration(options.plan_duration, total_started),
    );
    0
}

#[allow(dead_code)]
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
            planned.selection_basis.rust,
        );
        metrics.total_duration = total_started.elapsed();
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    Ok(0)
}

#[allow(dead_code)]
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
        planned.selection_basis.rust,
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
    if outcome.phase != ExecutionPhase::NoWork {
        crate::test_runner::emit_stage_time("python", outcome.phase_duration);
    }
    if outcome.index_rebuild_duration.as_millis() > 0 {
        crate::test_runner::emit_stage_time(
            "selective_index_repair",
            outcome.index_rebuild_duration,
        );
    }
}

fn record_rust_outcome(metrics: &mut LocalRubricMetrics, outcome: LanguagePhaseOutcome) {
    match outcome.phase {
        ExecutionPhase::Population(_) => {
            metrics.rust_population.summary = outcome.summary;
            metrics.rust_population.duration = outcome.phase_duration;
            metrics.rust_index_rebuild_duration += outcome.index_rebuild_duration;
            crate::test_runner::emit_stage_time("rust_population", outcome.phase_duration);
            if outcome.index_rebuild_duration.as_millis() > 0 {
                crate::test_runner::emit_stage_time(
                    "selective_index_repair",
                    outcome.index_rebuild_duration,
                );
            }
        }
        ExecutionPhase::Selective(_) => {
            metrics.rust_final.summary = outcome.summary;
            metrics.rust_final.duration = outcome.phase_duration;
            metrics.rust_index_rebuild_duration += outcome.index_rebuild_duration;
            crate::test_runner::emit_stage_time("rust_final", outcome.phase_duration);
            if outcome.index_rebuild_duration.as_millis() > 0 {
                crate::test_runner::emit_stage_time(
                    "selective_index_repair",
                    outcome.index_rebuild_duration,
                );
            }
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

#[allow(dead_code)]
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
    language_has_work(planned, kiss::Language::Python)
        || language_has_work(planned, kiss::Language::Rust)
}

pub(crate) fn language_has_work(planned: &PlannedSelectors, language: kiss::Language) -> bool {
    match language {
        kiss::Language::Python => {
            planned.population_required.python || !planned.sel.python.is_empty()
        }
        kiss::Language::Rust => planned.population_required.rust || !planned.sel.rust.is_empty(),
    }
}

pub(crate) fn execute_one_language(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    language: kiss::Language,
) -> Result<LanguagePhaseOutcome, String> {
    let python = runners::python_backer::PythonModule::for_execution_with_args(
        &planned.repo_root,
        &planned.ignore,
        options.extras.python,
    );
    let rust = runners::rust_backer::RustModule::for_execution(&planned.repo_root, &planned.ignore);
    let module: &dyn LanguageTestModule = match language {
        kiss::Language::Python => &python,
        kiss::Language::Rust => &rust,
    };
    let ctx = RunContext { planned, options };
    let phase = execution_phase(module, &ctx)?;
    execute_language_phase(module, &phase, &ctx)
}

pub(crate) fn merge_language_planned(
    repo_root: std::path::PathBuf,
    ignore: Vec<String>,
    python: Option<PlannedSelectors>,
    rust: Option<PlannedSelectors>,
) -> PlannedSelectors {
    let mut planned = crate::test_runner::empty_planned(repo_root, ignore);
    if let Some(py) = python {
        planned.sel.python = py.sel.python;
        planned.population_required.python = py.population_required.python;
        planned.source_paths.python = py.source_paths.python;
        planned.vcs_source_paths.python = py.vcs_source_paths.python;
        planned.snapshot_delta_modified.python = py.snapshot_delta_modified.python;
        planned.snapshot_delta_structural.python = py.snapshot_delta_structural.python;
        planned.prior_failure_selectors.python = py.prior_failure_selectors.python;
        planned.selection_basis.python = py.selection_basis.python;
        planned.skip_index_rebuild_after_selective.python =
            py.skip_index_rebuild_after_selective.python;
        planned.workspace_files_fingerprint = py
            .workspace_files_fingerprint
            .or(planned.workspace_files_fingerprint);
        planned.coverage_decision_engine_used |= py.coverage_decision_engine_used;
    }
    if let Some(rs) = rust {
        planned.sel.rust = rs.sel.rust;
        planned.population_required.rust = rs.population_required.rust;
        planned.source_paths.rust = rs.source_paths.rust;
        planned.vcs_source_paths.rust = rs.vcs_source_paths.rust;
        planned.snapshot_delta_modified.rust = rs.snapshot_delta_modified.rust;
        planned.snapshot_delta_structural.rust = rs.snapshot_delta_structural.rust;
        planned.prior_failure_selectors.rust = rs.prior_failure_selectors.rust;
        planned.selection_basis.rust = rs.selection_basis.rust;
        planned.skip_index_rebuild_after_selective.rust =
            rs.skip_index_rebuild_after_selective.rust;
        planned.workspace_files_fingerprint = rs
            .workspace_files_fingerprint
            .or(planned.workspace_files_fingerprint);
        planned.coverage_decision_engine_used |= rs.coverage_decision_engine_used;
    }
    planned
}

pub(crate) fn print_joined_dry_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Result<(), String> {
    let python = runners::python_backer::PythonModule::for_execution_with_args(
        &planned.repo_root,
        &planned.ignore,
        options.extras.python,
    );
    let rust = runners::rust_backer::RustModule::for_execution(&planned.repo_root, &planned.ignore);
    let ctx = RunContext { planned, options };
    let python_phase = execution_phase(&python, &ctx)?;
    let rust_phase = execution_phase(&rust, &ctx)?;
    let modules: [(&dyn LanguageTestModule, ExecutionPhase); 2] =
        [(&python, python_phase), (&rust, rust_phase)];
    print_dry_run(options, &modules)
}

pub(crate) fn finish_joined_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    process_started: Instant,
    python: Option<LanguagePhaseOutcome>,
    rust: Option<LanguagePhaseOutcome>,
) -> Result<i32, String> {
    if !planned_has_work(planned) && python.is_none() && rust.is_none() {
        return Ok(finish_no_work(planned, options, process_started));
    }
    let python_phase = python
        .as_ref()
        .map(|outcome| outcome.phase.clone())
        .unwrap_or(ExecutionPhase::NoWork);
    let rust_phase = rust
        .as_ref()
        .map(|outcome| outcome.phase.clone())
        .unwrap_or(ExecutionPhase::NoWork);
    let mut metrics = LocalRubricMetrics::new(
        planned,
        options,
        population_selector_count(&python_phase),
        matches!(rust_phase, ExecutionPhase::Population(_)),
        population_selector_count(&rust_phase),
        selective_selector_count(&rust_phase),
        planned.selection_basis.rust,
    );
    if let Some(outcome) = python {
        record_language_outcome(&mut metrics, kiss::Language::Python, outcome);
    }
    if let Some(outcome) = rust {
        record_language_outcome(&mut metrics, kiss::Language::Rust, outcome);
    }
    let mut code = metrics.python.summary.exit_code;
    code = runners::merge_exit_codes(code, rust_exit_code(&metrics, &rust_phase));
    Ok(finish_run_metrics(
        metrics,
        code,
        process_started,
        planned,
        options,
    ))
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
    if options.metrics {
        metrics.capture_cache_shape(&planned.repo_root);
        metrics.print();
    }
    let aggregate = FinalTestSummary::absorb(&[
        &metrics.python.summary,
        &metrics.rust_population.summary,
        &metrics.rust_final.summary,
    ]);
    print_final_test_summary(
        &aggregate,
        summary_total_duration(options.plan_duration, total_started),
    );
    code
}

fn summary_total_duration(_plan_duration: Duration, total_started: Instant) -> Duration {
    total_started.elapsed()
}

#[cfg(test)]
mod joined_run_test {
    use super::{LanguagePhaseOutcome, finish_joined_run, summary_total_duration};
    use crate::test_runner::SelectorRunOptions;
    use crate::test_runner::test_mode_fixtures::empty_planned_selectors;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn recap_duration_ignores_plan_duration_argument() {
        let started = Instant::now();
        std::thread::sleep(Duration::from_millis(30));
        let elapsed = summary_total_duration(Duration::from_secs(10), started);
        assert!(elapsed < Duration::from_secs(2), "got {elapsed:?}");
        assert!(elapsed >= Duration::from_millis(20), "got {elapsed:?}");
    }

    #[test]
    fn exit_code_merge_python_fail_rust_pass_and_reverse() {
        let planned = empty_planned_selectors(PathBuf::from("."));
        let options = SelectorRunOptions {
            dry_run: false,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: &[],
                rust: &[],
            },
            plan_duration: Duration::ZERO,
            gate: kiss::GateConfig::default(),
        };
        let py_fail = finish_joined_run(
            &planned,
            &options,
            Instant::now(),
            Some(LanguagePhaseOutcome::test_selective(1)),
            Some(LanguagePhaseOutcome::test_selective(0)),
        )
        .unwrap();
        assert_eq!(py_fail, 1);
        let rs_fail = finish_joined_run(
            &planned,
            &options,
            Instant::now(),
            Some(LanguagePhaseOutcome::test_selective(0)),
            Some(LanguagePhaseOutcome::test_selective(2)),
        )
        .unwrap();
        assert_eq!(rs_fail, 2);
    }
}

#[cfg(test)]
#[path = "run_logic_tests/mod.rs"]
mod tests;
