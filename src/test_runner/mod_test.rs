use crate::test_git::TestChangeMode;

use kiss::Language;

#[test]
fn run_test_returns_nonzero_when_planning_fails_outside_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::Commit,
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
fn run_test_dry_run_commit_in_workspace_completes() {
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::Commit,
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
fn run_test_reports_run_selectors_error_for_unsupported_rust_extra() {
    let extra = ["--format".to_string()];
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::Commit,
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
            invocation: match mode {
                TestChangeMode::Commit => crate::bin_cli::args::TestInvocation::Commit,
                TestChangeMode::Base => crate::bin_cli::args::TestInvocation::Base,
                TestChangeMode::Main => crate::bin_cli::args::TestInvocation::Main,
            },
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
        invocation: crate::bin_cli::args::TestInvocation::Base,
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
fn plan_all_requires_nonempty_language_populations() {
    let both = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        None,
    )
    .unwrap();
    assert!(both.python_population_required);
    assert!(both.rust_population_required);
    assert!(!both.py_sel.is_empty());
    assert!(!both.rs_sel.is_empty());

    let python_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Python),
    )
    .unwrap();
    assert!(python_only.python_population_required);
    assert!(!python_only.rust_population_required);
    assert!(!python_only.py_sel.is_empty());
    assert!(python_only.rs_sel.is_empty());

    let rust_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    assert!(!rust_only.python_population_required);
    assert!(rust_only.rust_population_required);
    assert!(rust_only.py_sel.is_empty());
    assert!(!rust_only.rs_sel.is_empty());
}

#[test]
fn plan_repo_root_target_matches_all_via_dot() {
    let cwd = std::env::current_dir().unwrap();
    let repo_root = crate::test_git::git_repo_root(&cwd).unwrap();
    let root = repo_root.canonicalize().unwrap();
    let via_all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    let via_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[root.to_string_lossy().into_owned()]),
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    let via_dot = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[".".into()]),
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    assert_eq!(via_all.rs_sel, via_root.rs_sel);
    assert_eq!(via_all.py_sel, via_root.py_sel);
    assert_eq!(via_all.rust_population_required, via_root.rust_population_required);
    assert_eq!(
        via_all.python_population_required,
        via_root.python_population_required
    );
    assert_eq!(via_all.rs_sel, via_dot.rs_sel);
    assert_eq!(via_all.py_sel, via_dot.py_sel);
    assert_eq!(via_all.rust_population_required, via_dot.rust_population_required);
    assert_eq!(
        via_all.python_population_required,
        via_dot.python_population_required
    );
    assert!(via_all.rust_population_required);
    assert!(!via_all.python_population_required);
    assert!(!via_all.coverage_decision_engine_used);
    assert!(!via_root.coverage_decision_engine_used);
    assert!(!via_dot.coverage_decision_engine_used);
}

#[test]
fn plan_subdirectory_is_not_workspace_enumerator() {
    let planned = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&["src/bin_cli".into()]),
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    let all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    assert!(!planned.rs_sel.is_empty());
    assert!(!planned.rust_source_paths.is_empty());
    assert!(planned.coverage_decision_engine_used);
    assert!(all.rust_source_paths.is_empty());
    assert!(!all.coverage_decision_engine_used);
    assert!(all.rust_population_required);
    // Partial targets stay on the decision-engine path (not planned_all). The engine
    // may still request population for cold/missing coverage; that is not All broadening.
}

#[test]
fn plan_dot_all_from_nested_cwd_stays_repo_wide() {
    let cwd = std::env::current_dir().unwrap();
    let nested = cwd.join("src/bin_cli");
    assert!(nested.is_dir());
    std::env::set_current_dir(&nested).unwrap();
    let planned = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Rust),
    );
    std::env::set_current_dir(&cwd).unwrap();
    let planned = planned.unwrap();
    let from_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        &[],
        Some(Language::Rust),
    )
    .unwrap();
    assert_eq!(planned.repo_root, from_root.repo_root);
    assert_eq!(planned.rs_sel, from_root.rs_sel);
}
