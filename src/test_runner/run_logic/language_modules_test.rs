use super::*;
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::{PlannedSelectors, SelectorRunOptions};
use std::path::PathBuf;
use std::time::Duration;

fn planned() -> PlannedSelectors {
    PlannedSelectors {
        repo_root: PathBuf::from("."),
        py_sel: vec!["tests/test_app.py::test_ok".to_string()],
        rs_sel: vec!["crate::tests::test_ok".to_string()],
        python_population_required: false,
        python_population_selectors: vec!["tests/test_app.py::test_population".to_string()],
        rust_population_selectors: vec!["crate::tests::test_population".to_string()],
        rust_source_paths: Vec::new(),
        rust_source_population_paths: Vec::new(),
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        ignore: Vec::new(),
    }
}

fn options() -> SelectorRunOptions<'static> {
    SelectorRunOptions {
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        plan_duration: Duration::ZERO,
    }
}

#[test]
#[allow(non_snake_case)]
fn PythonModule_policy_reads_python_selectors() {
    // Covers PythonModule population/selective selector policy.
    let mut planned = planned();
    planned.python_population_required = true;
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let module = PythonModule::for_execution();
    assert!(<PythonModule as LanguageExecutor>::population_required(
        &module, &ctx
    ));
    assert_eq!(
        <PythonModule as LanguageExecutor>::population_selectors(&module, &ctx).unwrap(),
        vec!["tests/test_app.py::test_population".to_string()]
    );
    assert_eq!(
        <PythonModule as LanguageExecutor>::selective_selectors(&module, &ctx),
        vec!["tests/test_app.py::test_ok".to_string()]
    );
}

#[test]
#[allow(non_snake_case)]
fn RustModule_policy_reads_rust_selectors() {
    // Covers RustModule population/selective selector policy.
    let mut planned = planned();
    planned.rust_source_population_paths = vec![PathBuf::from("src/lib.rs")];
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let module = RustModule::for_execution();
    assert!(<RustModule as LanguageExecutor>::population_required(
        &module, &ctx
    ));
    assert_eq!(
        <RustModule as LanguageExecutor>::population_selectors(&module, &ctx).unwrap(),
        vec!["crate::tests::test_population".to_string()]
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::selective_selectors(&module, &ctx),
        vec!["crate::tests::test_ok".to_string()]
    );
}

#[test]
fn language_executor_methods_handle_empty_runs_and_rebuild_indexes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let python = PythonModule::for_execution();
    let rust = RustModule::for_execution();

    assert_eq!(
        <PythonModule as LanguageExecutor>::language(&python),
        kiss::Language::Python
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::language(&rust),
        kiss::Language::Rust
    );
    assert_eq!(
        <PythonModule as LanguageExecutor>::run_population(&python, &[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );
    assert_eq!(
        <PythonModule as LanguageExecutor>::run_selective(&python, &[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::run_population(&rust, &[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::run_selective(&rust, &[], &ctx).unwrap(),
        SelectorExecutionSummary::default()
    );

    <PythonModule as LanguageExecutor>::rebuild_index(&python, &ctx).unwrap();
    <RustModule as LanguageExecutor>::rebuild_index(&rust, &ctx).unwrap();
    <PythonModule as LanguageExecutor>::write_manifest(&python, &[], &ctx).unwrap();
    <RustModule as LanguageExecutor>::write_manifest(&rust, &[], &ctx).unwrap();
}

#[test]
#[allow(non_snake_case)]
fn PythonModule_and_RustModule_execution_constructors_expose_language_policy() {
    let python = PythonModule::for_execution();
    let rust = RustModule::for_execution();

    assert_eq!(
        <PythonModule as LanguageExecutor>::language(&python),
        kiss::Language::Python
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::language(&rust),
        kiss::Language::Rust
    );
}

#[test]
fn language_executor_non_empty_runs_validate_jobs_before_spawning() {
    let planned = planned();
    let mut options = options();
    options.jobs = 0;
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };
    let python = PythonModule::for_execution();
    let rust = RustModule::for_execution();

    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            <PythonModule as LanguageExecutor>::run_population(
                &python,
                &["tests/test_app.py::test_value".to_string()],
                &ctx,
            )
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            <PythonModule as LanguageExecutor>::run_selective(
                &python,
                &["tests/test_app.py::test_value".to_string()],
                &ctx,
            )
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            <RustModule as LanguageExecutor>::run_population(
                &rust,
                &["crate::tests::test_value".to_string()],
                &ctx,
            )
        }))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            <RustModule as LanguageExecutor>::run_selective(
                &rust,
                &["crate::tests::test_value".to_string()],
                &ctx,
            )
        }))
        .is_err()
    );
}
