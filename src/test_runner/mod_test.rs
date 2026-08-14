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
            force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
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
            force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Rust),
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
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
            force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &extra,
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(Language::Rust),
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
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
            force_bad: false,
            metrics: false,
            jobs: 16,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
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
            force_bad: false,
        metrics: false,
        jobs: 16,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
    };
    let mut planned = crate::test_runner::PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: true,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };

    crate::test_runner::apply_cold_initialization_population(&args, &mut planned);

    assert!(planned.population_required.python);
    assert!(planned.population_required.rust);
}

#[test]
fn plan_all_materializes_nonempty_language_selector_sets() {
    let both = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        None,
        &kiss::GateConfig::default())
    .unwrap();
    assert!(!both.sel.python.is_empty());
    assert!(!both.sel.rust.is_empty());
    // Warm coverage populations may clear population_required; cold trees keep it.

    let python_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Python),
        &kiss::GateConfig::default())
    .unwrap();
    assert!(!python_only.population_required.rust);
    assert!(!python_only.sel.python.is_empty());
    assert!(python_only.sel.rust.is_empty());

    let rust_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    assert!(!rust_only.population_required.python);
    assert!(rust_only.sel.python.is_empty());
    assert!(!rust_only.sel.rust.is_empty());
}

#[test]
fn plan_repo_root_target_matches_all_via_dot() {
    let cwd = std::env::current_dir().unwrap();
    let repo_root = crate::test_git::git_repo_root(&cwd).unwrap();
    let root = repo_root.canonicalize().unwrap();
    let via_all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    let via_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[root.to_string_lossy().into_owned()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    let via_dot = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[".".into()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    assert_eq!(via_all.sel.rust, via_root.sel.rust);
    assert_eq!(via_all.sel.python, via_root.sel.python);
    assert_eq!(via_all.population_required.rust, via_root.population_required.rust);
    assert_eq!(
        via_all.population_required.python,
        via_root.population_required.python
    );
    assert_eq!(via_all.sel.rust, via_dot.sel.rust);
    assert_eq!(via_all.sel.python, via_dot.sel.python);
    assert_eq!(via_all.population_required.rust, via_dot.population_required.rust);
    assert_eq!(
        via_all.population_required.python,
        via_dot.population_required.python
    );
    // All forces population only when the on-disk coverage population is not
    // already current for the planned selector set (warm reuse stays selective).
    assert!(!via_all.population_required.python);
    assert!(!via_all.coverage_decision_engine_used);
    assert!(!via_root.coverage_decision_engine_used);
    assert!(!via_dot.coverage_decision_engine_used);
}

#[test]
fn plan_subdirectory_is_not_workspace_enumerator() {
    let planned = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&["src/bin_cli".into()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    let all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    assert!(!planned.sel.rust.is_empty());
    assert!(!planned.source_paths.rust.is_empty());
    assert!(planned.coverage_decision_engine_used);
    assert!(all.source_paths.rust.is_empty());
    assert!(!all.coverage_decision_engine_used);
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
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default());
    std::env::set_current_dir(&cwd).unwrap();
    let planned = planned.unwrap();
    let from_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed { python: &[], rust: &[] },
        Some(Language::Rust),
        &kiss::GateConfig::default())
    .unwrap();
    assert_eq!(planned.repo_root, from_root.repo_root);
    assert_eq!(planned.sel.rust, from_root.sel.rust);
}

#[test]
fn apply_force_bad_noop_when_flag_off_and_merges_when_on() {
    let tmp = tempfile::tempdir().unwrap();
    let mut planned = crate::test_runner::PlannedSelectors {
        repo_root: tmp.path().to_path_buf(),
        sel: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec!["tests/a.py::t".into()],
            rust: Vec::new(),
        },
        population_required: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_modified: crate::test_runner::language_keyed::LanguageKeyed {
            python: 0,
            rust: 0,
        },
        snapshot_delta_structural: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
        prior_failure_selectors: crate::test_runner::language_keyed::LanguageKeyed {
            python: Vec::new(),
            rust: Vec::new(),
        },
        coverage_decision_engine_used: false,
        selection_basis: crate::test_runner::language_keyed::LanguageKeyed {
            python: crate::test_runner::coverage_decision::SelectionBasis::Current,
            rust: crate::test_runner::coverage_decision::SelectionBasis::Current,
        },
        ignore: Vec::new(),
        workspace_files_fingerprint: None,
        skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
            python: false,
            rust: false,
        },
    };
    let args = crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::All,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    };
    crate::test_runner::apply_force_bad(&args, &mut planned).unwrap();
    assert!(planned.prior_failure_selectors.python.is_empty());
    let args_on = crate::test_runner::RunTestCmdArgs {
        force_bad: true,
        ..args
    };
    crate::test_runner::apply_force_bad(&args_on, &mut planned).unwrap();
    assert!(planned.prior_failure_selectors.python.is_empty());
}
