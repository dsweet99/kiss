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
            metrics: false,
            jobs: 1,
            extra: &[],
            python_extra: &[],
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
            rust_vcs_source_paths: 0,
            rust_snapshot_delta_modified: 0,
            rust_snapshot_delta_structural: false,
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
            python_extra: &[],
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
            python_extra: &[],
            plan_duration: Duration::ZERO,
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
                extra: &[],
                python_extra: &[],
                lang_filter: None,
                config_main_branch: None,
            }).unwrap();
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
                python_extra: &[],
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
            rust_vcs_source_paths: 0,
            rust_snapshot_delta_modified: 0,
            rust_snapshot_delta_structural: false,
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
                python_extra: &[],
                plan_duration: Duration::ZERO,
            },
        )
        .unwrap_err();

        assert!(err.contains("jobs"));
    }
}
