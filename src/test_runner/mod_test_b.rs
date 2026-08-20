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
