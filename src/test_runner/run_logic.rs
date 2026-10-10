use super::{PlannedSelectors, SelectorRunOptions, runners};
use crate::test_runner::final_summary::{FinalTestSummary, print_final_test_summary};
use crate::test_runner::language_keyed::LanguageKeyed;
use crate::test_runner::test_selection::{LanguageExecutor, LanguageTestModule, RunContext};
use std::time::{Duration, Instant};

#[path = "run_logic/cache_decision_metrics.rs"]
mod cache_decision_metrics;
#[path = "run_logic/language_executor.rs"]
mod language_executor;
#[cfg(test)]
#[path = "run_logic/language_modules.rs"]
mod language_modules;
#[path = "run_logic/metrics.rs"]
mod metrics;
pub(crate) use language_executor::LanguagePhaseOutcome;
use language_executor::{
    ExecutionPhase, execute_language_phase, execution_phase, population_selector_count,
    print_dry_run, selective_selector_count,
};
use metrics::LocalRubricMetrics;

fn execution_modules(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Vec<Box<dyn LanguageTestModule>> {
    crate::test_runner::lang_registry::languages()
        .into_iter()
        .map(|language| {
            crate::test_runner::lang_registry::execution_module(language, planned, options)
        })
        .collect()
}

fn planned_phases<'m>(
    modules: &'m [Box<dyn LanguageTestModule>],
    ctx: &RunContext<'_, '_>,
) -> Result<Vec<(&'m dyn LanguageTestModule, ExecutionPhase)>, String> {
    modules
        .iter()
        .map(|module| Ok((module.as_ref(), execution_phase(module.as_ref(), ctx)?)))
        .collect()
}

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
    let owned = execution_modules(planned, &options);
    let modules = planned_phases(&owned, &ctx)?;
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
    crate::test_runner::emit_test_progress(runners::NO_SELECTED_TESTS_MSG);
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

fn keyed_phases(
    modules: &[(&dyn LanguageTestModule, ExecutionPhase)],
) -> LanguageKeyed<ExecutionPhase> {
    let mut phases = LanguageKeyed::<ExecutionPhase>::default();
    for (module, phase) in modules {
        *phases.get_mut(LanguageExecutor::language(*module)) = phase.clone();
    }
    phases
}

fn metrics_for_phases(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    phases: &LanguageKeyed<ExecutionPhase>,
) -> LocalRubricMetrics {
    LocalRubricMetrics::new(
        planned,
        options,
        population_selector_count(&phases.python),
        matches!(phases.rust, ExecutionPhase::Population(_)),
        population_selector_count(&phases.rust),
        selective_selector_count(&phases.rust),
        planned.selection_basis.rust,
    )
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
        let mut metrics = metrics_for_phases(planned, options, &keyed_phases(modules));
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
    let mut metrics = metrics_for_phases(planned, options, &keyed_phases(modules));
    let ctx = RunContext { planned, options };
    let mut code = 0;
    for (module, phase) in modules {
        let outcome = execute_language_phase(*module, phase, &ctx)?;
        code = runners::merge_exit_codes(code, outcome.summary.exit_code);
        record_language_outcome(&mut metrics, *module, outcome);
    }
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
    module: &dyn LanguageTestModule,
    outcome: LanguagePhaseOutcome,
) {
    if outcome.phase == ExecutionPhase::NoWork {
        return;
    }
    let population = matches!(outcome.phase, ExecutionPhase::Population(_));
    let stage = module.stage_label(population);
    let (slot, index_rebuild) = metrics.stage_mut(stage);
    slot.summary = outcome.summary;
    slot.duration = outcome.phase_duration;
    *index_rebuild += outcome.index_rebuild_duration;
    crate::test_runner::emit_stage_time(stage, outcome.phase_duration);
    if outcome.index_rebuild_duration.as_millis() > 0 {
        crate::test_runner::emit_stage_time(
            "selective_index_repair",
            outcome.index_rebuild_duration,
        );
    }
}

fn planned_has_work(planned: &PlannedSelectors) -> bool {
    crate::test_runner::lang_registry::languages()
        .into_iter()
        .any(|language| language_has_work(planned, language))
}

pub(crate) fn language_has_work(planned: &PlannedSelectors, language: kiss::Language) -> bool {
    *planned.population_required.get(language) || !planned.sel.get(language).is_empty()
}

pub(crate) fn execute_one_language(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    language: kiss::Language,
) -> Result<LanguagePhaseOutcome, String> {
    let module = crate::test_runner::lang_registry::execution_module(language, planned, options);
    let ctx = RunContext { planned, options };
    let phase = execution_phase(module.as_ref(), &ctx)?;
    execute_language_phase(module.as_ref(), &phase, &ctx)
}

pub(crate) fn merge_language_planned(
    repo_root: std::path::PathBuf,
    ignore: Vec<String>,
    by_language: LanguageKeyed<Option<PlannedSelectors>>,
) -> PlannedSelectors {
    let mut planned = crate::test_runner::empty_planned(repo_root, ignore);
    let mut by_language = by_language;
    for language in kiss::Language::ALL {
        if let Some(from) = by_language.get_mut(language) {
            take_language_plan(&mut planned, from, language);
        }
    }
    planned
}

fn take_language_plan(to: &mut PlannedSelectors, from: &mut PlannedSelectors, l: kiss::Language) {
    macro_rules! take_keyed {
        ($($field:ident),*) => {
            $(std::mem::swap(to.$field.get_mut(l), from.$field.get_mut(l));)*
        };
    }
    take_keyed!(
        sel,
        population_required,
        source_paths,
        vcs_source_paths,
        prior_failure_selectors,
        selection_basis,
        skip_index_rebuild_after_selective
    );
    to.workspace_files_fingerprint = from
        .workspace_files_fingerprint
        .take()
        .or(to.workspace_files_fingerprint.take());
    to.selection_engine_used |= from.selection_engine_used;
}

pub(crate) fn print_joined_dry_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
) -> Result<(), String> {
    let ctx = RunContext { planned, options };
    let owned = execution_modules(planned, options);
    let modules = planned_phases(&owned, &ctx)?;
    print_dry_run(options, &modules)
}

pub(crate) fn finish_joined_run(
    planned: &PlannedSelectors,
    options: &SelectorRunOptions<'_>,
    process_started: Instant,
    jobs: LanguageKeyed<Option<LanguagePhaseOutcome>>,
) -> Result<i32, String> {
    let mut jobs = jobs;
    let languages = crate::test_runner::lang_registry::languages();
    let no_outcomes = languages
        .iter()
        .all(|language| jobs.get(*language).is_none());
    if !planned_has_work(planned) && no_outcomes {
        return Ok(finish_no_work(planned, options, process_started));
    }
    let phases = LanguageKeyed::from_fn(|language| {
        jobs.get(language)
            .as_ref()
            .map(|outcome| outcome.phase.clone())
            .unwrap_or_default()
    });
    let mut metrics = metrics_for_phases(planned, options, &phases);
    let owned = execution_modules(planned, options);
    let mut code = 0;
    for module in &owned {
        if let Some(outcome) = jobs
            .get_mut(LanguageExecutor::language(module.as_ref()))
            .take()
        {
            code = runners::merge_exit_codes(code, outcome.summary.exit_code);
            record_language_outcome(&mut metrics, module.as_ref(), outcome);
        }
    }
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
    use crate::test_runner::language_keyed::LanguageKeyed;
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
    fn merge_language_planned_takes_each_language_from_its_own_plan() {
        use crate::test_runner::empty_planned;
        let root = std::path::PathBuf::from("/r");
        let mut py = empty_planned(root.clone(), Vec::new());
        py.sel.python = vec!["t.py::a".into()];
        py.sel.rust = vec!["ignored".into()];
        py.population_required.python = true;
        py.workspace_files_fingerprint = Some("py".into());
        let mut rs = empty_planned(root.clone(), Vec::new());
        rs.sel.rust = vec!["crate::b".into()];
        rs.vcs_source_paths.rust = 3;
        rs.workspace_files_fingerprint = Some("rs".into());
        rs.selection_engine_used = true;
        let merged = super::merge_language_planned(
            root,
            Vec::new(),
            LanguageKeyed {
                python: Some(py),
                rust: Some(rs),
            },
        );
        assert_eq!(merged.sel.python, vec!["t.py::a".to_string()]);
        assert_eq!(merged.sel.rust, vec!["crate::b".to_string()]);
        assert!(merged.population_required.python);
        assert!(!merged.population_required.rust);
        assert_eq!(merged.vcs_source_paths.rust, 3);
        assert_eq!(merged.workspace_files_fingerprint.as_deref(), Some("rs"));
        assert!(merged.selection_engine_used);
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
            crate::test_runner::language_keyed::LanguageKeyed {
                python: Some(LanguagePhaseOutcome::test_selective(1)),
                rust: Some(LanguagePhaseOutcome::test_selective(0)),
            },
        )
        .unwrap();
        assert_eq!(py_fail, 1);
        let rs_fail = finish_joined_run(
            &planned,
            &options,
            Instant::now(),
            crate::test_runner::language_keyed::LanguageKeyed {
                python: Some(LanguagePhaseOutcome::test_selective(0)),
                rust: Some(LanguagePhaseOutcome::test_selective(2)),
            },
        )
        .unwrap();
        assert_eq!(rs_fail, 2);
    }
}

#[cfg(test)]
#[path = "run_logic_tests/mod.rs"]
mod tests;
