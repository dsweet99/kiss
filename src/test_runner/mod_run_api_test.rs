use std::path::PathBuf;
use std::time::Duration;

use super::*;

impl RunTestCmdArgs<'_> {
    fn dry_run_commit() -> Self {
        Self {
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
            gate_config: kiss::GateConfig::default(),
        }
    }
}

impl PlannedSelectors {
    fn empty(repo_root: PathBuf) -> Self {
        Self {
            repo_root,
            sel: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec![],
                rust: vec![],
            },
            population_required: crate::test_runner::language_keyed::LanguageKeyed {
                python: false,
                rust: false,
            },
            source_paths: crate::test_runner::language_keyed::LanguageKeyed {
                python: Vec::new(),
                rust: vec![],
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
            selection_basis: Default::default(),
            ignore: vec![],
            workspace_files_fingerprint: None,
            skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
                python: false,
                rust: false,
            },
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
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: &[],
                rust: &[],
            },
            plan_duration: Duration::ZERO,
            gate: kiss::GateConfig::default(),
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
    planned.sel.rust = vec!["tests::case".to_string()];
    let extra = vec!["--format".to_string(), "json".to_string()];

    let err = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: true,
            force_rerun: false,
metrics: false,
            jobs: 1,
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: &[],
                rust: &extra,
            },
            plan_duration: Duration::ZERO,
        gate: kiss::GateConfig::default()
        },
    )
    .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--format"));
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
            plan_selectors(PlanSelectorsRequest {
                mode: TestChangeMode::Commit,
                main_branch_cli: None,
                base_branch_cli: None,
                ignore: &[],
                extras: crate::test_runner::language_keyed::LanguageKeyed {
                    python: &[],
                    rust: &[],
                },
                lang_filter: None,
                config_main_branch: None,
            }).unwrap();
        std::env::set_current_dir(orig).unwrap();
        assert_eq!(planned.repo_root, tmp.path().canonicalize().unwrap());
        assert!(planned.sel.python.is_empty());
        assert!(planned.sel.rust.is_empty());
        let code = run_selectors(
            &planned,
            SelectorRunOptions {
                dry_run: true,
                force_rerun: false,
metrics: false,
                jobs: 1,
                extras: crate::test_runner::language_keyed::LanguageKeyed {
                    python: &[],
                    rust: &[],
                },
                plan_duration: Duration::ZERO,
            gate: kiss::GateConfig::default()
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
            sel: crate::test_runner::language_keyed::LanguageKeyed {
                python: vec!["tests/test_app.py::test_ok".to_string()],
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
            selection_basis: Default::default(),
            ignore: Vec::new(),
            workspace_files_fingerprint: None,
            skip_index_rebuild_after_selective: crate::test_runner::language_keyed::LanguageKeyed {
                python: false,
                rust: false,
            },
        };

        let err = run_selectors(
            &planned,
            SelectorRunOptions {
                dry_run: false,
                force_rerun: false,
metrics: false,
                jobs: 0,
                extras: crate::test_runner::language_keyed::LanguageKeyed {
                    python: &[],
                    rust: &[],
                },
                plan_duration: Duration::ZERO,
            gate: kiss::GateConfig::default()
            },
        )
        .unwrap_err();

        assert!(err.contains("jobs"));
    }
}
