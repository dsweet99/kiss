use crate::bin_cli::args::TestInvocation;
use kiss::Language;

struct ForcePopCase {
    invocation: TestInvocation,
    lang_filter: Option<Language>,
    has_py: bool,
    has_rs: bool,
    expect_py_pop: bool,
    expect_rs_pop: bool,
}

#[test]
fn apply_force_all_population_only_for_all_invocation() {
    let cases = [
        ForcePopCase {
            invocation: TestInvocation::All,
            lang_filter: None,
            has_py: true,
            has_rs: true,
            expect_py_pop: true,
            expect_rs_pop: true,
        },
        ForcePopCase {
            invocation: TestInvocation::All,
            lang_filter: Some(Language::Python),
            has_py: true,
            has_rs: true,
            expect_py_pop: true,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::All,
            lang_filter: Some(Language::Rust),
            has_py: true,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: true,
        },
        ForcePopCase {
            invocation: TestInvocation::All,
            lang_filter: None,
            has_py: true,
            has_rs: false,
            expect_py_pop: true,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::All,
            lang_filter: None,
            has_py: false,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: true,
        },
        ForcePopCase {
            invocation: TestInvocation::Targets(vec!["tests/a.py::t".into()]),
            lang_filter: None,
            has_py: true,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::Commit,
            lang_filter: None,
            has_py: true,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::Base,
            lang_filter: None,
            has_py: true,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::Main,
            lang_filter: None,
            has_py: true,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::Targets(vec!["tests/a.py::t".into()]),
            lang_filter: Some(Language::Python),
            has_py: true,
            has_rs: false,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
        ForcePopCase {
            invocation: TestInvocation::Commit,
            lang_filter: Some(Language::Rust),
            has_py: false,
            has_rs: true,
            expect_py_pop: false,
            expect_rs_pop: false,
        },
    ];

    for case in &cases {
        let tmp = tempfile::tempdir().unwrap();
        let mut planned = crate::test_runner::PlannedSelectors {
            repo_root: tmp.path().to_path_buf(),
            sel: crate::test_runner::language_keyed::LanguageKeyed {
                python: if case.has_py {
                    vec!["tests/a.py::t".into()]
                } else {
                    Vec::new()
                },
                rust: if case.has_rs {
                    vec!["crate::tests::t".into()]
                } else {
                    Vec::new()
                },
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
            invocation: case.invocation.clone(),
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: false,
            force_rerun: true,
            force_bad: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
            ignore: &[],
            lang_filter: case.lang_filter,
            config_main_branch: None,
            gate_config: kiss::GateConfig::default(),
        };
        crate::test_runner::apply_force_all_population(&args, &mut planned);
        assert_eq!(
            planned.population_required.python, case.expect_py_pop,
            "python_population_required for {:?} lang={:?}",
            case.invocation, case.lang_filter
        );
        assert_eq!(
            planned.population_required.rust, case.expect_rs_pop,
            "rust_population_required for {:?} lang={:?}",
            case.invocation, case.lang_filter
        );
    }
}
