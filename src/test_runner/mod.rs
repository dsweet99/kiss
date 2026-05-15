mod runners;

use std::path::PathBuf;

use kiss::Language;

use crate::test_git::TestChangeMode;

pub struct RunTestCmdArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
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
        config_main_branch,
    } = a;
    match plan_selectors(
        mode,
        main_branch_cli,
        base_branch_cli,
        ignore,
        lang_filter,
        config_main_branch,
    ) {
        Ok(planned) => match run_selectors(&planned, dry_run, extra) {
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
    crate::test_git::assert_git_repo(&cwd).map_err(|e| format!("error: kiss test requires a git repository ({e})"))?;
    let repo_root = crate::test_git::git_repo_root(&cwd)?;
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
        lang_filter.map(|l| match l {
            Language::Python => crate::test_git::TestLangFilter::Python,
            Language::Rust => crate::test_git::TestLangFilter::Rust,
        }),
    );
    let (source_changed, test_changed) = runners::partition_changed_paths(&abs_paths);
    let (py_sel, rs_sel) =
        runners::combined_selectors(&repo_root, &source_changed, &test_changed, lang_filter, &ignore_norm)?;
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
) -> Result<i32, String> {
    if planned.py_sel.is_empty() && planned.rs_sel.is_empty() {
        println!("{}", runners::NO_COVERING_TESTS_MSG);
        return Ok(0);
    }
    let py_argv = runners::build_pytest_argv(&planned.py_sel, extra);
    let rs_argv = runners::build_cargo_test_argv(&planned.rs_sel, extra);
    if dry_run {
        if !planned.py_sel.is_empty() {
            println!("{}", runners::shell_quote_line(&py_argv));
        }
        if !planned.rs_sel.is_empty() {
            println!("{}", runners::shell_quote_line(&rs_argv));
        }
        return Ok(0);
    }
    let mut code = 0i32;
    if !planned.py_sel.is_empty() {
        code = runners::merge_exit_codes(code, runners::run_command_inherit(&py_argv, &planned.repo_root)?);
    }
    if !planned.rs_sel.is_empty() {
        code = runners::merge_exit_codes(code, runners::run_command_inherit(&rs_argv, &planned.repo_root)?);
    }
    Ok(code)
}

#[cfg(test)]
mod plan_tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::test_git::TestChangeMode;

    // See `git_command` in `src/test_git.rs` for why we scrub `GIT_*`
    // env vars: pre-commit exports `GIT_INDEX_FILE` to isolate hooks
    // and would otherwise redirect every test's git call to the real
    // repo's index.
    fn git_in(dir: &Path) -> Command {
        crate::test_git::git_command(dir)
    }

    fn init(tmp: &TempDir) {
        assert!(git_in(tmp.path())
            .arg("init")
            .status()
            .unwrap()
            .success());
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
        git_in(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap();
        git_in(tmp.path())
            .args(["commit", "-m", "m"])
            .status()
            .unwrap();
        std::fs::write(tmp.path().join("b.py"), "y=1\n").unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let planned = plan_selectors(
            TestChangeMode::Commit,
            None,
            None,
            &[],
            None,
            None,
        )
        .unwrap();
        std::env::set_current_dir(orig).unwrap();
        assert_eq!(planned.repo_root, tmp.path().canonicalize().unwrap());
        assert!(planned.py_sel.is_empty());
        assert!(planned.rs_sel.is_empty());
        let code = run_selectors(&planned, true, &[]).unwrap();
        assert_eq!(code, 0);
    }
}

#[cfg(test)]
#[path = "runners_test.rs"]
mod runners_test;
