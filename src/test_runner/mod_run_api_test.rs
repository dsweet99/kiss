use std::path::PathBuf;
use std::time::Duration;

use super::*;

impl RunTestCmdArgs<'_> {
    fn dry_run_commit() -> Self {
        Self {
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
        }
    }
}

impl PlannedSelectors {
    fn empty(repo_root: PathBuf) -> Self {
        Self {
            repo_root,
            py_sel: vec![],
            rs_sel: vec![],
            python_population_required: false,
            rust_population_required: false,
            rust_source_paths: vec![],
            python_prior_failure_selectors: Vec::new(),
            rust_prior_failure_selectors: Vec::new(),
            coverage_decision_engine_used: true,
            rust_selection_basis: Default::default(),
            ignore: vec![],
        }
    }
}

impl SelectorRunOptions<'_> {
    fn dry_run() -> Self {
        Self {
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            plan_duration: Duration::ZERO,
        }
    }
}

#[test]
fn run_selectors_accepts_empty_plan() {
    let planned = PlannedSelectors::empty(std::env::current_dir().unwrap_or_default());

    let code = run_selectors(&planned, SelectorRunOptions::dry_run()).unwrap();

    assert_eq!(code, 0);
}

#[test]
fn dry_run_rejects_unsupported_rust_test_args_without_panic() {
    let mut planned = PlannedSelectors::empty(std::env::current_dir().unwrap_or_default());
    planned.rs_sel = vec!["tests::case".to_string()];
    let extra = vec!["--format".to_string(), "json".to_string()];

    let err = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &extra,
            plan_duration: Duration::ZERO,
        },
    )
    .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--format"));
}

#[test]
fn validate_selection_accepts_tiny_recall_fixture_execution() {
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
    let code = super::validate_selection(args);

    assert_eq!(code, 2);

    let extra_args = ["--exact".to_string()];
    let ignore_args = ["target".to_string()];
    let planning_args = ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        jobs: 1,
        extra: &extra_args,
        ignore: &ignore_args,
        lang_filter: Some(Language::Rust),
        fixture: None,
        config_main_branch: None,
    };
    assert_eq!(planning_args.change_mode(), TestChangeMode::Commit);
    assert_eq!(planning_args.main_branch_arg(), None);
    assert_eq!(planning_args.base_branch_arg(), None);
    assert_eq!(planning_args.normalized_lang_filter(), Some(Language::Rust));
    assert_eq!(planning_args.planning_extra_args(), extra_args);
    assert_eq!(planning_args.planning_ignore_args(), ignore_args);
    assert!(planning_args.is_dry_run());
    assert_eq!(planning_args.fixture_name(), None);
    assert!(planning_args.has_positive_jobs());

    let fixture_args = ValidateSelectionCmdArgs {
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
    };
    assert!(fixture_args.validate_dry_run_request().is_ok());

    let zero_jobs_args = ValidateSelectionCmdArgs {
        mode: TestChangeMode::Commit,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        jobs: 0,
        extra: &[],
        ignore: &[],
        lang_filter: None,
        fixture: None,
        config_main_branch: None,
    };
    assert_eq!(
        zero_jobs_args.validate_dry_run_request().unwrap_err(),
        "error: kiss test validate-selection jobs must be greater than zero"
    );
}

#[test]
fn validation_report_totals_and_print_are_consistent() {
    let report = ValidationReport {
        selected_python: 2,
        selected_rust: 3,
        full_python: 4,
        full_rust: 6,
        python_population_required: false,
        rust_population_required: false,
    };

    assert_eq!(report.selected_total(), 5);
    assert_eq!(report.full_total(), 10);
    assert_eq!(report.selected_for_language(Language::Python), 2);
    assert_eq!(report.selected_for_language(Language::Rust), 3);
    assert_eq!(report.full_for_language(Language::Python), 4);
    assert_eq!(report.full_for_language(Language::Rust), 6);
    assert!(report.has_selected_tests());
    assert!(report.has_full_universe());
    assert_eq!(report.selection_ratio(), Some(0.5));
    assert!(!report.rust_population_required());
    report.print(true);

    let empty = ValidationReport {
        selected_python: 0,
        selected_rust: 0,
        full_python: 0,
        full_rust: 0,
        python_population_required: false,
        rust_population_required: true,
    };
    assert_eq!(empty.selection_ratio(), None);
    assert!(!empty.has_selected_tests());
    assert!(!empty.has_full_universe());
    assert!(empty.rust_population_required());
    empty.print(true);
}

#[test]
fn validation_report_counts_rust_population_as_selected() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    let planned = PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        py_sel: Vec::new(),
        rs_sel: Vec::new(),
        python_population_required: false,
        rust_population_required: true,
        rust_source_paths: vec![lib],
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
        ignore: Vec::new(),
    };

    let report = super::validation_report(&planned, Some(Language::Rust)).unwrap();

    assert_eq!(report.selected_rust, 1);
    assert_eq!(report.full_rust, 1);
    assert!(report.rust_population_required);
}

#[test]
fn validation_report_counts_python_population_as_selected() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("tests")).unwrap();
    std::fs::write(tmp.path().join("app.py"), "def value():\n    return 1\n").unwrap();
    std::fs::write(
        tmp.path().join("tests").join("test_app.py"),
        "from app import value\n\n\
def test_value():\n    assert value() == 1\n",
    )
    .unwrap();
    let planned = PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        py_sel: Vec::new(),
        rs_sel: Vec::new(),
        python_population_required: true,
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
        ignore: Vec::new(),
    };

    let report = super::validation_report(&planned, Some(Language::Python)).unwrap();

    assert_eq!(report.selected_python, 1);
    assert_eq!(report.full_python, 1);
    assert!(report.python_population_required);
}

#[test]
fn validation_report_counts_planned_selectors_in_full_universe() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() {} }\n",
    )
    .unwrap();
    let planned = PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        py_sel: Vec::new(),
        rs_sel: vec!["external_changed_test".to_string()],
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: Vec::new(),
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: Default::default(),
        ignore: Vec::new(),
    };

    let report = super::validation_report(&planned, Some(Language::Rust)).unwrap();

    assert_eq!(report.selected_rust, 1);
    assert_eq!(report.full_rust, 2);
    assert_eq!(report.selection_ratio(), Some(0.5));
}

#[test]
fn run_test_rejects_non_git_directory_quickly() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = run_test(RunTestCmdArgs::dry_run_commit());
    std::env::set_current_dir(orig).unwrap();
    assert_eq!(code, 1);
}

mod plan_tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn git_in(dir: &Path) -> Command {
        crate::test_git::git_command(dir)
    }

    fn init(tmp: &TempDir) {
        assert!(git_in(tmp.path()).arg("init").status().unwrap().success());
        git_in(tmp.path())
            .args(["config", "user.email", "t@t.t"])
            .status()
            .unwrap();
        git_in(tmp.path())
            .args(["config", "user.name", "t"])
            .status()
            .unwrap();
    }

    #[test]
    fn plan_selectors_commit_smoke() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = TempDir::new().unwrap();
        init(&tmp);
        std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
        git_in(tmp.path()).args(["add", "."]).status().unwrap();
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap();
        std::fs::write(tmp.path().join("b.py"), "y=1\n").unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let planned: PlannedSelectors =
            plan_selectors(TestChangeMode::Commit, None, None, &[], &[], None, None).unwrap();
        std::env::set_current_dir(orig).unwrap();
        assert_eq!(planned.repo_root, tmp.path().canonicalize().unwrap());
        assert!(planned.py_sel.is_empty());
        assert!(planned.rs_sel.is_empty());
        let code = run_selectors(
            &planned,
            SelectorRunOptions {
                dry_run: true,
                force_rerun: false,
                metrics: false,
                jobs: 1,
                extra: &[],
                plan_duration: Duration::ZERO,
            },
        )
        .unwrap();
        assert_eq!(code, 0);
        assert!(planned.coverage_decision_engine_used);
    }

    #[test]
    fn run_selectors_rejects_zero_jobs() {
        let tmp = TempDir::new().unwrap();
        let planned = PlannedSelectors {
            repo_root: tmp.path().to_path_buf(),
            py_sel: vec!["tests/test_app.py::test_ok".to_string()],
            rs_sel: Vec::new(),
            python_population_required: false,
            rust_population_required: false,
            rust_source_paths: Vec::new(),
            python_prior_failure_selectors: Vec::new(),
            rust_prior_failure_selectors: Vec::new(),
            coverage_decision_engine_used: true,
            rust_selection_basis: Default::default(),
            ignore: Vec::new(),
        };

        let err = run_selectors(
            &planned,
            SelectorRunOptions {
                dry_run: false,
                force_rerun: false,
                metrics: false,
                jobs: 0,
                extra: &[],
                plan_duration: Duration::ZERO,
            },
        )
        .unwrap_err();

        assert!(err.contains("jobs"));
    }
}
