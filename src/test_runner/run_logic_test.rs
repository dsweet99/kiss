use super::*;
use kiss::Language;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::test_runner::runners::SelectorExecutionSummary;

fn planned() -> PlannedSelectors {
    PlannedSelectors {
        repo_root: PathBuf::from("."),
        py_sel: Vec::new(),
        rs_sel: Vec::new(),
        python_population_required: false,
        python_population_selectors: Vec::new(),
        rust_population_selectors: Vec::new(),
        rust_source_paths: Vec::new(),
        rust_source_population_paths: Vec::new(),
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        ignore: Vec::new(),
    }
}

fn options(force_rerun: bool) -> SelectorRunOptions<'static> {
    SelectorRunOptions {
        dry_run: true,
        force_rerun,
        metrics: false,
        jobs: 1,
        extra: &[],
        plan_duration: Duration::ZERO,
    }
}

#[test]
fn force_rerun_does_not_make_rust_population_required() {
    let mut planned = planned();
    planned.rust_source_paths = vec![PathBuf::from("src/lib.rs")];
    planned.rs_sel = vec!["crate::tests::test_selected".to_string()];
    let options = options(true);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(matches!(
        execution_phase(&ExecutionModule::Rust, &ctx).unwrap(),
        ExecutionPhase::Selective(_)
    ));
}

#[test]
fn rust_population_requirement_comes_from_population_paths() {
    let mut planned = planned();
    planned.rust_source_paths = vec![PathBuf::from("src/lib.rs")];
    planned.rust_source_population_paths = vec![PathBuf::from("src/lib.rs")];
    planned.rust_population_selectors = vec!["crate::tests::test_population".to_string()];
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(matches!(
        execution_phase(&ExecutionModule::Rust, &ctx).unwrap(),
        ExecutionPhase::Population(selectors) if selectors == vec!["crate::tests::test_population".to_string()]
    ));
}

#[test]
fn rust_dry_run_is_population_xor_selective() {
    let selective = vec!["crate::selective_test".to_string()];
    let mut planned = planned();
    planned.rs_sel = selective.clone();
    planned.rust_source_population_paths = vec![PathBuf::from("src/lib.rs")];
    planned.rust_population_selectors = vec!["crate::population_test".to_string()];
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(matches!(
        execution_phase(&ExecutionModule::Rust, &ctx).unwrap(),
        ExecutionPhase::Population(_)
    ));

    planned.rust_source_population_paths.clear();
    planned.rust_population_selectors.clear();
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };
    assert_eq!(
        execution_phase(&ExecutionModule::Rust, &ctx).unwrap(),
        ExecutionPhase::Selective(selective)
    );
}

#[test]
fn language_modules_expose_language_and_indexable_source_policy() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn value() {}\n").unwrap();

    assert!(language_modules::python_is_indexable_source(
        &tmp.path().join("app.py"),
        tmp.path()
    ));
    assert!(!language_modules::python_is_indexable_source(
        Path::new("<frozen importlib>"),
        tmp.path()
    ));
    assert!(language_modules::rust_is_indexable_source(
        &tmp.path().join("src").join("lib.rs"),
        tmp.path()
    ));
    assert!(!language_modules::rust_is_indexable_source(
        Path::new(".kiss/runtime.rs"),
        tmp.path()
    ));
}

#[test]
fn empty_module_runs_return_default_summaries_without_spawning() {
    let planned = planned();
    let options = options(false);
    let ctx = RunContext {
        planned: &planned,
        options: &options,
    };

    assert_eq!(
        language_modules::run_rslip_selectors_for_module(&[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );
    assert_eq!(
        language_modules::run_rust_selectors_for_module(&[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );
    let outcome: LanguagePhaseOutcome =
        execute_language_phase(&ExecutionModule::Python, &ExecutionPhase::NoWork, &ctx).unwrap();
    assert_eq!(outcome.summary, SelectorExecutionSummary::default());
    assert!(matches!(outcome.phase, ExecutionPhase::NoWork));
}

#[test]
fn python_module_reports_python_policy() {
    assert_eq!(Language::Python, kiss::Language::Python);
}

#[test]
fn rust_module_reports_rust_policy() {
    assert_eq!(Language::Rust, kiss::Language::Rust);
}

#[test]
fn language_phase_outcome_carries_phase_summary_and_timings() {
    let outcome = LanguagePhaseOutcome {
        phase: ExecutionPhase::NoWork,
        summary: SelectorExecutionSummary::default(),
        phase_duration: Duration::ZERO,
        index_rebuild_duration: Duration::ZERO,
    };

    assert!(matches!(outcome.phase, ExecutionPhase::NoWork));
    assert_eq!(outcome.summary.total, 0);
    assert_eq!(outcome.phase_duration, Duration::ZERO);
    assert_eq!(outcome.index_rebuild_duration, Duration::ZERO);
}

#[test]
fn python_outcome_records_index_rebuild_duration_in_metrics() {
    let planned = planned();
    let options = options(false);
    let mut metrics = LocalRubricMetrics::new(&planned, &options, false, 0, 0);
    let outcome = LanguagePhaseOutcome {
        phase: ExecutionPhase::Selective(vec!["tests/test_app.py::test_ok".to_string()]),
        summary: SelectorExecutionSummary {
            total: 1,
            cache_hits: 0,
            cache_misses: 1,
            failed: 0,
            exit_code: 0,
        },
        phase_duration: Duration::from_millis(7),
        index_rebuild_duration: Duration::from_millis(3),
    };

    record_python_outcome(&mut metrics, outcome);

    assert_eq!(metrics.python.summary.total, 1);
    assert_eq!(metrics.python.duration, Duration::from_millis(7));
    assert_eq!(
        metrics.python_index_rebuild_duration,
        Duration::from_millis(3)
    );
}
