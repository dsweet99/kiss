use super::*;
use crate::test_runner::PlannedSelectors;
use crate::test_runner::coverage_decision::LanguagePlanner;
use crate::test_runner::runners::SelectorExecutionSummary;
use std::path::PathBuf;

fn planned() -> PlannedSelectors {
    let mut planned =
        crate::test_runner::test_mode_fixtures::empty_planned_selectors(PathBuf::from("."));
    planned.sel.python = vec!["tests/test_app.py::test_ok".to_string()];
    planned.sel.rust = vec!["crate::tests::test_ok".to_string()];
    planned
}

#[test]
#[allow(non_snake_case)]
fn python_module_run_population_uses_temp_repo_kernel() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.sel.python.clear();
    let options = super::dry_run_selector_options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };
    let module = PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    let _ = <PythonModule as LanguageExecutor>::run_population(&module, &[], &ctx);
}

#[test]
#[allow(non_snake_case)]
fn PythonModule_policy_reads_python_population_decision() {
    let mut planned = planned();
    planned.population_required.python = true;
    let options = super::dry_run_selector_options();
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
    planned.population_required.rust = true;
    let options = super::dry_run_selector_options();
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
    let options = super::dry_run_selector_options();
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
fn python_rebuild_index_skips_when_pure_test_operand_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.skip_index_rebuild_after_selective.python = true;
    let options = super::dry_run_selector_options();
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };
    let python = PythonModule::for_execution(&planned.repo_root, &planned.ignore);
    <PythonModule as LanguageExecutor>::rebuild_index(&python, &ctx).unwrap();
    assert!(
        !tmp.path().join(".kiss").exists(),
        "pure test-operand selective plans must not publish a Python coverage index"
    );
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
    assert_eq!(rust_lines[0], "RUST POPULATION");
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
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    let mut options = super::dry_run_selector_options();
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
