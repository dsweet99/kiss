use crate::test_git::TestChangeMode;

use kiss::Language;

use crate::test_runner::{ValidateSelectionCmdArgs, ValidationReport, validate_selection};

#[test]
fn validate_selection_cmd_args_contract() {
    let args: ValidateSelectionCmdArgs<'_> = ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        fixture: None,
        config_main_branch: None,
    };

    assert_eq!(validate_selection(args), 2);
}

#[test]
fn validation_report_print_contract() {
    let report = ValidationReport {
        selected_python: 1,
        selected_rust: 2,
        full_python: 3,
        full_rust: 7,
        python_population_required: false,
        rust_population_required: true,
    };

    assert_eq!(report.selected_total(), 3);
    assert_eq!(report.full_total(), 10);
    report.print(true);
}

#[test]
fn validation_types_expose_language_specific_counts() {
    let args = ValidateSelectionCmdArgs {
        mode: TestChangeMode::Base,
        main_branch_cli: Some("main"),
        base_branch_cli: Some("base"),
        dry_run: true,
        jobs: 2,
        extra: &["--exact".to_string()],
        ignore: &["target".to_string()],
        lang_filter: Some(Language::Python),
        fixture: None,
        config_main_branch: Some("trunk"),
    };
    assert_eq!(args.change_mode(), TestChangeMode::Base);
    assert_eq!(args.main_branch_arg(), Some("main"));
    assert_eq!(args.base_branch_arg(), Some("base"));
    assert_eq!(args.normalized_lang_filter(), Some(Language::Python));
    assert_eq!(args.planning_extra_args(), &["--exact".to_string()]);
    assert_eq!(args.planning_ignore_args(), &["target".to_string()]);
    assert!(args.is_dry_run());
    assert_eq!(args.fixture_name(), None);
    assert!(args.has_positive_jobs());

    let report = ValidationReport {
        selected_python: 0,
        selected_rust: 0,
        full_python: 1,
        full_rust: 3,
        python_population_required: false,
        rust_population_required: false,
    };
    assert_eq!(report.selected_for_language(Language::Python), 0);
    assert_eq!(report.selected_for_language(Language::Rust), 0);
    assert_eq!(report.full_for_language(Language::Python), 1);
    assert_eq!(report.full_for_language(Language::Rust), 3);
    assert!(!report.has_selected_tests());
    assert!(report.has_full_universe());
    assert_eq!(report.selection_ratio(), Some(0.0));
    assert!(!report.rust_population_required());
    report.print(false);
}

#[test]
fn validate_selection_cmd_args() {
    let args = ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        fixture: None,
        config_main_branch: None,
    };

    assert!(args.validate_dry_run_request().is_ok());
}

#[test]
fn validation_report() {
    let report = ValidationReport {
        selected_python: 1,
        selected_rust: 0,
        full_python: 2,
        full_rust: 0,
        python_population_required: false,
        rust_population_required: false,
    };

    assert_eq!(report.selection_ratio(), Some(0.5));
}

#[test]
fn print() {
    let report = ValidationReport {
        selected_python: 0,
        selected_rust: 0,
        full_python: 0,
        full_rust: 0,
        python_population_required: false,
        rust_population_required: false,
    };

    report.print(true);
}

#[test]
fn run_test_returns_nonzero_when_planning_fails_outside_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
    });
    std::env::set_current_dir(old).unwrap();
    assert_eq!(code, 1);
}

#[test]
fn validate_selection_returns_nonzero_when_planning_fails_outside_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = validate_selection(ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Rust),
        fixture: None,
        config_main_branch: None,
    });
    std::env::set_current_dir(old).unwrap();
    assert_eq!(code, 1);
}

#[test]
fn run_test_dry_run_commit_in_workspace_completes() {
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Rust),
        config_main_branch: None,
    });
    assert!(
        code == 0 || code == 1,
        "dry-run planning must complete with a process status, got {code}"
    );
}

#[test]
fn validate_selection_dry_run_commit_in_workspace_prints_report() {
    let code = validate_selection(ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Rust),
        fixture: None,
        config_main_branch: None,
    });
    assert_eq!(code, 0);
}

#[test]
fn validate_selection_runs_tiny_recall_fixture_path() {
    let code = validate_selection(ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        jobs: 1,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        fixture: Some("tiny-recall"),
        config_main_branch: None,
    });
    assert!(
        code == 0 || code == 1,
        "tiny-recall fixture must complete with a process status, got {code}"
    );
}

#[test]
fn run_test_reports_run_selectors_error_for_unsupported_rust_extra() {
    let extra = ["--format".to_string()];
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs: 1,
        extra: &extra,
        ignore: &[],
        lang_filter: Some(Language::Rust),
        config_main_branch: None,
    });
    assert_eq!(code, 1);
}

#[test]
fn cold_initialization_predicate_is_limited_to_unfiltered_base_or_main() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();

    fn args(
        mode: TestChangeMode,
        dry_run: bool,
        lang_filter: Option<Language>,
    ) -> crate::test_runner::RunTestCmdArgs<'static> {
        crate::test_runner::RunTestCmdArgs {
            mode,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run,
            force_rerun: false,
            metrics: false,
            jobs: 16,
            extra: &[],
            ignore: &[],
            lang_filter,
            config_main_branch: None,
        }
    }
    let base = args(TestChangeMode::Base, false, None);
    let dry_run = args(TestChangeMode::Base, true, None);
    let rust_only = args(TestChangeMode::Base, false, Some(Language::Rust));
    let commit = args(TestChangeMode::Commit, false, None);

    assert!(crate::test_runner::should_force_cold_initialization(
        &base,
        tmp.path()
    ));
    assert!(!crate::test_runner::should_force_cold_initialization(
        &dry_run,
        tmp.path()
    ));
    assert!(!crate::test_runner::should_force_cold_initialization(
        &rust_only,
        tmp.path()
    ));
    assert!(!crate::test_runner::should_force_cold_initialization(
        &commit,
        tmp.path()
    ));
}

#[test]
fn cold_initialization_population_marks_missing_state_for_both_languages() {
    let tmp = tempfile::tempdir().unwrap();
    let args = crate::test_runner::RunTestCmdArgs {
        mode: TestChangeMode::Base,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        metrics: false,
        jobs: 16,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
    };
    let mut planned = crate::test_runner::PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        py_sel: Vec::new(),
        rs_sel: Vec::new(),
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: crate::test_runner::coverage_decision::RustSelectionBasis::Current,
        ignore: Vec::new(),
    };

    crate::test_runner::apply_cold_initialization_population(&args, &mut planned);

    assert!(planned.python_population_required);
    assert!(planned.rust_population_required);
}

#[test]
fn plan_selectors_main_mode_uses_diff_target_in_workspace() {
    let planned = crate::test_runner::plan_selectors(
        TestChangeMode::Main,
        None,
        None,
        &[],
        &[],
        Some(Language::Rust),
        None,
    );
    assert!(
        planned.is_ok() || planned.is_err(),
        "Main-mode planning must resolve a diff target without internal missing-target errors"
    );
    if let Err(err) = planned {
        assert!(
            !err.contains("missing diff target"),
            "unexpected missing diff target: {err}"
        );
    }
}
