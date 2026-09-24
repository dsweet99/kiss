use std::fs;

use crate::test_git::TestChangeMode;

use kiss::Language;

#[test]
fn plan_for_invocation_follows_target_focus() {
    use crate::bin_cli::args::TestInvocation;
    use crate::test_runner::test_mode_fixtures::{dry_run_cmd_args, init_git, with_cwd};
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    init_git(&tmp);
    std::fs::write(tmp.path().join("lib.rs"), "pub fn f() {}\n").unwrap();
    crate::test_runner::test_mode_fixtures::git_in(tmp.path())
        .args(["add", "."])
        .status()
        .unwrap();
    crate::test_runner::test_mode_fixtures::git_in(tmp.path())
        .args(["commit", "-m", "m"])
        .status()
        .unwrap();
    with_cwd(tmp.path(), || {
        let all = dry_run_cmd_args(TestInvocation::All, &[], 1, Some(Language::Rust));
        assert!(super::plan_for_invocation(&all).is_ok());
        let commit = dry_run_cmd_args(TestInvocation::Commit, &[], 1, Some(Language::Rust));
        assert!(super::plan_for_invocation(&commit).is_ok());
        let missing = dry_run_cmd_args(
            TestInvocation::Targets(vec!["missing.py".into()]),
            &[],
            1,
            Some(Language::Python),
        );
        assert!(super::plan_for_invocation(&missing).is_err());
    });
}

#[test]
fn run_test_returns_nonzero_when_planning_fails_outside_git_repo() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let old = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let code = crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
        invocation: crate::bin_cli::args::TestInvocation::Commit,
        target_request: crate::test_runner::target_request::request_from_focus(
            crate::test_runner::target_request::TargetFocus::Git(
                crate::test_runner::target_request::GitFocus::Commit,
            ),
            None,
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        coverage_all: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    });
    std::env::set_current_dir(old).unwrap();
    assert_eq!(code, 1);
}

#[test]
fn run_test_dry_run_commit_in_workspace_completes() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    let code = crate::test_runner::test_mode_fixtures::with_cwd(tmp.path(), || {
        crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::Commit,
            target_request: crate::test_runner::target_request::request_from_focus(
                crate::test_runner::target_request::TargetFocus::Git(
                    crate::test_runner::target_request::GitFocus::Commit,
                ),
                Some(Language::Rust),
                &[],
            ),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter: Some(Language::Rust),
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        })
    });
    assert!(
        code == 0 || code == 1,
        "dry-run planning must complete with a process status, got {code}"
    );
}

#[test]
fn run_test_reports_run_selectors_error_for_unsupported_rust_extra() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    let extra = ["--format".to_string()];
    let code = crate::test_runner::test_mode_fixtures::with_cwd(tmp.path(), || {
        crate::test_runner::run_test(crate::test_runner::RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::All,
            target_request: crate::test_runner::target_request::workspace_request(
                Some(Language::Rust),
                &[],
            ),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
            jobs: 1,
            extra: &extra,
            python_extra: &[],
            ignore: &[],
            lang_filter: Some(Language::Rust),
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        })
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
        let invocation = match mode {
            TestChangeMode::Commit => crate::bin_cli::args::TestInvocation::Commit,
            TestChangeMode::Base => crate::bin_cli::args::TestInvocation::Base,
            TestChangeMode::Main => crate::bin_cli::args::TestInvocation::Main,
        };
        crate::test_runner::RunTestCmdArgs {
            invocation: invocation.clone(),
            target_request: crate::test_runner::target_request::request_from_invocation(
                &invocation,
                None,
                None,
                None,
                lang_filter,
                &[],
            ),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run,
            force_rerun: false,
            force_bad: false,
            metrics: false,
            coverage_all: false,
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
        target_request: crate::test_runner::target_request::request_from_invocation(
            &crate::bin_cli::args::TestInvocation::Base,
            None,
            None,
            None,
            None,
            &[],
        ),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        coverage_all: false,
        jobs: 16,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: None,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
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
        vcs_source_paths: crate::test_runner::language_keyed::LanguageKeyed { python: 0, rust: 0 },
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
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn ok() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    fs::write(root.join("app.py"), "def f():\n    return 1\n").unwrap();
    fs::write(
        root.join("test_app.py"),
        "from app import f\n\ndef test_f():\n    assert f() == 1\n",
    )
    .unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(root).unwrap();
    let both = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        None,
        &kiss::GateConfig::default(),
    );
    let python_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Python),
        &kiss::GateConfig::default(),
    );
    let rust_only = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    std::env::set_current_dir(orig).unwrap();
    let both = both.unwrap();
    assert!(!both.sel.python.is_empty());
    assert!(!both.sel.rust.is_empty());

    let python_only = python_only.unwrap();
    assert!(!python_only.population_required.rust);
    assert!(!python_only.sel.python.is_empty());
    assert!(python_only.sel.rust.is_empty());

    let rust_only = rust_only.unwrap();
    assert!(!rust_only.population_required.python);
    assert!(rust_only.sel.python.is_empty());
    assert!(!rust_only.sel.rust.is_empty());
}

#[test]
fn plan_repo_root_target_matches_all_via_dot() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn ok() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let root = root.canonicalize().unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    let via_all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    let via_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[root.to_string_lossy().into_owned()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    let via_dot = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&[".".into()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    std::env::set_current_dir(orig).unwrap();
    let via_all = via_all.unwrap();
    let via_root = via_root.unwrap();
    let via_dot = via_dot.unwrap();
    assert_eq!(via_all.sel.rust, via_root.sel.rust);
    assert_eq!(via_all.sel.python, via_root.sel.python);
    assert_eq!(
        via_all.population_required.rust,
        via_root.population_required.rust
    );
    assert_eq!(
        via_all.population_required.python,
        via_root.population_required.python
    );
    assert_eq!(via_all.sel.rust, via_dot.sel.rust);
    assert_eq!(via_all.sel.python, via_dot.sel.python);
    assert_eq!(
        via_all.population_required.rust,
        via_dot.population_required.rust
    );
    assert_eq!(
        via_all.population_required.python,
        via_dot.population_required.python
    );

    assert!(!via_all.population_required.python);
    assert!(!via_all.coverage_decision_engine_used);
    assert!(!via_root.coverage_decision_engine_used);
    assert!(!via_dot.coverage_decision_engine_used);
}

#[test]
fn plan_subdirectory_is_not_workspace_enumerator() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::create_dir_all(root.join("src/bin_cli")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn root() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn root_ok() { assert_eq!(super::root(), 1); } }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/bin_cli/mod.rs"),
        "pub fn cli() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn cli_ok() { assert_eq!(super::cli(), 1); } }\n",
    )
    .unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(root).unwrap();
    let planned = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::Targets(&["src/bin_cli".into()]),
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    let all = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    std::env::set_current_dir(orig).unwrap();
    let planned = planned.unwrap();
    let all = all.unwrap();
    assert!(!planned.sel.rust.is_empty());
    assert!(!planned.source_paths.rust.is_empty());
    assert!(planned.coverage_decision_engine_used);
    assert!(all.source_paths.rust.is_empty());
    assert!(!all.coverage_decision_engine_used);
}

#[test]
fn plan_dot_all_from_nested_cwd_stays_repo_wide() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::create_dir_all(root.join("src/nested")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn ok() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    fs::write(root.join("src/nested/mod.rs"), "pub fn n() -> u32 { 1 }\n").unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(root)
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    let root = root.canonicalize().unwrap();
    let nested = root.join("src/nested");
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(&nested).unwrap();
    let planned = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    std::env::set_current_dir(&root).unwrap();
    let from_root = crate::test_runner::plan_target_selectors(
        crate::test_runner::TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    std::env::set_current_dir(orig).unwrap();
    let planned = planned.unwrap();
    let from_root = from_root.unwrap();
    assert_eq!(planned.repo_root, from_root.repo_root);
    assert_eq!(planned.sel.rust, from_root.sel.rust);
}
