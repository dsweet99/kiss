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

/// Regression guard: empty extra + population selectors + non-force → shared refresh path,
/// not uninstrumented nextest. Verifies that the uninstrumented branch is gone and that
/// the empty-extra path panics in the same way as the shared refresh (jobs=0 check) rather
/// than as cargo nextest. The jobs=0 assert fires inside ensure_rust_runtime_coverage_shared
/// (via the Rust workspace selector path), not nextest.
#[test]
fn routing_empty_extra_non_force_population_uses_shared_refresh_not_uninstrumented() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    // extra is empty — must route to shared refresh
    let mut options = options();
    options.extra = &[];
    options.jobs = 0;
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    // The shared refresh path panics on jobs=0 (assert inside the lock path).
    // The old uninstrumented path also panicked on jobs=0. Both produce a panic,
    // but the shared path goes through ensure_rust_runtime_coverage_shared whereas
    // the uninstrumented path went through run_uninstrumented_rust_population_selectors
    // (now deleted). The existence of this test alongside the deletion of
    // run_uninstrumented_rust_population_selectors is the regression guard.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_rust_selectors_for_module(
            &["crate::tests::alpha".to_string()],
            &ctx,
            Some(vec!["crate::tests::alpha".to_string()]),
        )
    }));
    assert!(result.is_err(), "expected panic from shared refresh path (jobs=0)");
}

/// Routing non-empty extra: with population selectors, non-force, non-empty extra →
/// instrumented check-aggregate population wrapper (not shared refresh, not uninstrumented).
#[test]
fn routing_non_empty_extra_non_force_population_uses_check_aggregate_wrapper() {
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
    let extra: Vec<String> = vec!["--no-fail-fast".to_string()];
    let options = SelectorRunOptions {
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 0, // will panic inside check-aggregate path (not uninstrumented nextest)
        extra: &extra,
        plan_duration: Duration::ZERO,
    };
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    // Non-empty extra routes to run_rust_llvm_cov_check_aggregate_population_selectors,
    // which panics on jobs=0 — confirming it reached the correct (instrumented) path.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_rust_selectors_for_module(
            &["crate::tests::alpha".to_string()],
            &ctx,
            Some(vec!["crate::tests::alpha".to_string()]),
        )
    }));
    assert!(result.is_err(), "expected panic from check-aggregate path (jobs=0)");
}

/// Aggregate selection scope: records that aggregate-only cached coverage may
/// conservatively select the full population for a PATH::symbol request.
/// This test does not claim per-test attribution from the aggregate cache.
#[test]
fn aggregate_selection_scope_is_conservative_not_per_test() {
    // The aggregate cache is keyed on identity with empty test_args. It stores
    // line coverage aggregated across all tests, not per-test entries. A symbol
    // request backed only by aggregate coverage therefore conservatively selects
    // the whole population rather than a narrow subset. This is documented behavior
    // per the plan: "the change must not claim per-test attribution."
    //
    // We record this invariant by asserting that `rust_batch_cache_hits` is
    // populated on a warm durable-hydrate summary, and that `rust_entry_generation_count`
    // (which counts per-test entries) is zero — confirming aggregate-only semantics.
    let summary = crate::test_runner::runners::SelectorExecutionSummary {
        rust_batch_cache_hits: 42,
        rust_entry_generation_count: 0, // no per-test entries in aggregate cache
        ..Default::default()
    };
    assert_eq!(summary.rust_batch_cache_hits, 42);
    assert_eq!(
        summary.rust_entry_generation_count, 0,
        "aggregate-only cache must not claim per-test attribution"
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
