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
        rust_population_required: false,
    };

    report.print(true);
}
