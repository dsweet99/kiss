use super::*;
use crate::test_runner::coverage_decision::LanguagePlanner;
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
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
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
fn PythonModule_policy_reads_python_population_decision() {
    let mut planned = planned();
    planned.python_population_required = true;
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let module = PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    assert!(<PythonModule as LanguageExecutor>::population_required(
        &module, &ctx
    ));
    assert_eq!(
        <PythonModule as LanguageExecutor>::selective_selectors(&module, &ctx),
        vec!["tests/test_app.py::test_ok".to_string()]
    );
}

#[test]
#[allow(non_snake_case)]
fn RustModule_policy_reads_rust_population_decision() {
    let mut planned = planned();
    planned.rust_population_required = true;
    let options = options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let module = RustModule::for_execution(&planned.repo_root, &planned.ignore);
    assert!(<RustModule as LanguageExecutor>::population_required(
        &module, &ctx
    ));
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

    let python = PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    let rust = RustModule::for_execution(&planned.repo_root, &planned.ignore);

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
    let root = PathBuf::from(".");
    let ignore = Vec::<String>::new();
    let python = PythonModule::for_execution(&root, &ignore);
    let rust = RustModule::for_execution(&root, &ignore);

    assert_eq!(
        <PythonModule as LanguageExecutor>::language(&python),
        kiss::Language::Python
    );
    assert_eq!(
        <RustModule as LanguageExecutor>::language(&rust),
        kiss::Language::Rust
    );
    assert_eq!(
        <PythonModule as LanguagePlanner>::language(&python),
        kiss::Language::Python
    );
    assert_eq!(
        <RustModule as LanguagePlanner>::language(&rust),
        kiss::Language::Rust
    );
}

#[test]
fn dry_run_lines_report_population_and_selector_commands() {
    let root = PathBuf::from(".");
    let ignore = Vec::<String>::new();
    let python = PythonModule::for_execution(&root, &ignore);
    let rust = RustModule::for_execution(&root, &ignore);

    let python_lines = <PythonModule as LanguageExecutor>::dry_run_lines(
        &python,
        &["tests/test_app.py::test_ok".to_string()],
        true,
        &["-q".to_string()],
        4,
    )
    .unwrap();
    assert_eq!(python_lines[0], "PYTHON COVERAGE POPULATION");
    assert_eq!(
        python_lines[1],
        "python '-m' pytest tests/test_app.py::test_ok '-q'"
    );

    let rust_lines = <RustModule as LanguageExecutor>::dry_run_lines(
        &rust,
        &["crate::tests::test_ok".to_string()],
        true,
        &[],
        4,
    )
    .unwrap();
    assert_eq!(rust_lines[0], "RUST COVERAGE POPULATION");
    assert!(
        rust_lines
            .iter()
            .any(|line| line == "RUST BATCH selectors=1 jobs=4")
    );
    assert!(
        rust_lines
            .iter()
            .any(|line| line == "RUST SELECTOR crate::tests::test_ok")
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
    let python = PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    let rust = RustModule::for_execution(&planned.repo_root, &planned.ignore);

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

#[test]
fn cached_rust_check_aggregate_is_only_for_non_forced_selective_runs() {
    assert!(should_try_cached_rust_check_aggregate(false, &None));
    assert!(!should_try_cached_rust_check_aggregate(true, &None));
    assert!(!should_try_cached_rust_check_aggregate(
        false,
        &Some(vec!["tests::population".to_string()])
    ));
}

#[test]
fn rust_execution_helper_reaches_check_aggregate_population_with_ignores() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.ignore = vec!["--ignore".to_string(), "other.rs".to_string()];
    let mut options = options();
    options.jobs = 0;
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_rust_selectors_for_module(
                &["tests::population".to_string()],
                &ctx,
                Some(vec!["tests::population".to_string()]),
            )
        }))
        .is_err()
    );
}

#[test]
fn rust_execution_helper_tries_cached_selective_before_falling_through() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn value() {}\n").unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    let mut options = options();
    options.jobs = 0;
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_rust_selectors_for_module(&["tests::case".to_string()], &ctx, None)
        }))
        .is_err()
    );
}
