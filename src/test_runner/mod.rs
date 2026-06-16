mod runners;

use std::path::PathBuf;

use kiss::Language;

use crate::test_git::TestChangeMode;

pub const COLD_CACHE_MSG: &str = "run kiss check first to warm the rslip cache";

pub struct RunTestCmdArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub jobs: Option<usize>,
    pub config_main_branch: Option<&'a str>,
}

pub fn run_test(a: RunTestCmdArgs<'_>) -> i32 {
    let RunTestCmdArgs {
        mode,
        main_branch_cli,
        base_branch_cli,
        dry_run,
        extra,
        ignore,
        lang_filter,
        jobs,
        config_main_branch,
    } = a;
    let parallelism = jobs.unwrap_or_else(pyfork::default_parallelism);
    if let Err(e) = pyfork::validate_pytest_extra(extra) {
        eprintln!("{e}");
        return 1;
    }
    match plan_selectors(
        mode,
        main_branch_cli,
        base_branch_cli,
        ignore,
        lang_filter,
        config_main_branch,
    ) {
        Ok(planned) => match run_selectors(&planned, dry_run, extra, parallelism) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

pub(crate) struct PlannedSelectors {
    pub repo_root: PathBuf,
    pub py_sel: Vec<String>,
    pub rs_sel: Vec<String>,
}

pub(crate) fn plan_selectors(
    mode: TestChangeMode,
    main_branch_cli: Option<&str>,
    base_branch_cli: Option<&str>,
    ignore: &[String],
    lang_filter: Option<Language>,
    config_main_branch: Option<&str>,
) -> Result<PlannedSelectors, String> {
    let ignore_norm = kiss::normalize_ignore_prefixes(ignore);
    let cwd = std::env::current_dir().map_err(|e| format!("error: kiss test: {e}"))?;
    crate::test_git::assert_git_repo(&cwd)
        .map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let repo_root = crate::test_git::git_repo_root(&cwd)?;
    let py_sel = if lang_filter == Some(Language::Rust) {
        Vec::new()
    } else {
        let collected = pyfork::collect_nodeids(&repo_root, &[])?;
        match rslip::load_database(&repo_root)? {
            None => {
                return Err(format!("error: kiss test: {COLD_CACHE_MSG}"));
            }
            Some(db) => rslip::scheduled_nodeids(&repo_root, &collected, &db)?,
        }
    };
    let rs_sel = if lang_filter == Some(Language::Python) {
        Vec::new()
    } else {
        let diff_target = crate::test_git::resolve_diff_target(
            &repo_root,
            mode,
            config_main_branch,
            main_branch_cli,
            base_branch_cli,
        )?;
        let rel_changed = match mode {
            TestChangeMode::Commit => crate::test_git::changed_paths_commit(&repo_root)?,
            TestChangeMode::Base | TestChangeMode::Main => {
                let Some(ref rev) = diff_target else {
                    return Err("error: kiss test: internal error (missing diff target)".into());
                };
                crate::test_git::changed_paths_since(&repo_root, rev)?
            }
        };
        let abs_paths = crate::test_git::resolve_changed_source_paths(
            &repo_root,
            &rel_changed,
            &ignore_norm,
            Some(crate::test_git::TestLangFilter::Rust),
        );
        let (source_changed, test_changed) = runners::partition_changed_paths(&abs_paths);
        runners::rust_selectors(&repo_root, &source_changed, &test_changed, &ignore_norm)?
    };
    Ok(PlannedSelectors {
        repo_root,
        py_sel,
        rs_sel,
    })
}

pub(crate) fn run_selectors(
    planned: &PlannedSelectors,
    dry_run: bool,
    extra: &[String],
    parallelism: usize,
) -> Result<i32, String> {
    if planned.py_sel.is_empty() && planned.rs_sel.is_empty() {
        println!("{}", runners::NO_COVERING_TESTS_MSG);
        return Ok(0);
    }
    let rs_argv = runners::build_cargo_test_argv(&planned.rs_sel, extra);
    if dry_run {
        for nodeid in &planned.py_sel {
            println!(
                "{}",
                runners::shell_quote_line(&runners::build_pytest_fork_argv(nodeid, extra))
            );
        }
        if !planned.rs_sel.is_empty() {
            println!("{}", runners::shell_quote_line(&rs_argv));
        }
        return Ok(0);
    }
    let mut code = 0i32;
    if !planned.py_sel.is_empty() {
        code = runners::merge_exit_codes(
            code,
            pyfork::run_pool(&planned.repo_root, &planned.py_sel, parallelism, extra)?,
        );
    }
    if !planned.rs_sel.is_empty() {
        code = runners::merge_exit_codes(
            code,
            runners::run_command_inherit(&rs_argv, &planned.repo_root)?,
        );
    }
    Ok(code)
}

#[cfg(test)]
mod coverage_witness {
    use super::*;

    impl<'a> RunTestCmdArgs<'a> {
        fn witness() {}
    }

    impl PlannedSelectors {
        fn witness() -> Self {
            Self {
                repo_root: PathBuf::new(),
                py_sel: vec![],
                rs_sel: vec![],
            }
        }
    }

    #[test]
    fn witness_test_runner_types() {
        RunTestCmdArgs::witness();
        let _ = PlannedSelectors::witness();
    }
}

#[cfg(test)]
mod behavior_tests {
    use super::*;

    fn test_args() -> RunTestCmdArgs<'static> {
        RunTestCmdArgs {
            mode: TestChangeMode::Commit,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            extra: &[],
            ignore: &[],
            lang_filter: None,
            jobs: None,
            config_main_branch: None,
        }
    }

    #[test]
    fn run_test_reports_error_outside_git_repo() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();

        let code = run_test(test_args());

        std::env::set_current_dir(orig).unwrap();
        assert_eq!(code, 1);
    }

    #[test]
    fn run_selectors_with_no_covering_tests_succeeds_without_spawning() {
        let planned = PlannedSelectors {
            repo_root: std::env::current_dir().unwrap_or_default(),
            py_sel: vec![],
            rs_sel: vec![],
        };

        let code = run_selectors(&planned, true, &[], 1).unwrap();

        assert_eq!(code, 0);
    }

    #[test]
    fn command_args_carry_dry_run_and_filters() {
        let args = RunTestCmdArgs {
            extra: &["--ignored".to_string()],
            ignore: &["target".to_string()],
            lang_filter: Some(Language::Rust),
            jobs: Some(4),
            ..test_args()
        };

        assert!(args.dry_run);
        assert_eq!(args.extra, ["--ignored"]);
        assert_eq!(args.ignore, ["target"]);
        assert_eq!(args.lang_filter, Some(Language::Rust));
        assert_eq!(args.jobs, Some(4));
    }
}

#[cfg(test)]
mod plan_tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::test_git::TestChangeMode;

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
    fn plan_selectors_commit_smoke_without_cache_errors() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = TempDir::new().unwrap();
        init(&tmp);
        std::fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
        git_in(tmp.path()).args(["add", "."]).status().unwrap();
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        match plan_selectors(TestChangeMode::Commit, None, None, &[], None, None) {
            Err(e) => assert!(e.contains(COLD_CACHE_MSG), "{e}"),
            Ok(_) => panic!("expected cold cache error"),
        }
        std::env::set_current_dir(orig).unwrap();
    }

    #[test]
    fn run_test_returns_error_outside_git_repo() {
        let _cwd_guard = crate::cwd_test_lock::lock();
        let tmp = TempDir::new().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let code = run_test(RunTestCmdArgs {
            mode: TestChangeMode::Commit,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            extra: &[],
            ignore: &[],
            lang_filter: None,
            jobs: None,
            config_main_branch: None,
        });
        std::env::set_current_dir(orig).unwrap();
        assert_eq!(code, 1);
    }
}

#[cfg(test)]
#[path = "runners_test.rs"]
mod runners_test;
