use std::fs;

use tempfile::TempDir;

use crate::test_git::TestChangeMode;
use crate::test_runner::test_mode_fixtures::{
    RS_COVERING_SELECTOR, assert_base_delta_plan, checkout_branch, edit_rust_covered_source,
    ensure_main_branch, git_in, git_stdout, init_git, warm_base_demo_with_historical_source,
    warm_committed_rust_demo, with_cwd,
};
use crate::test_runner::{PlannedSelectors, RunTestCmdArgs, plan_selectors, run_test};

fn plan(
    mode: TestChangeMode,
    main: Option<&str>,
    base: Option<&str>,
    lang: Option<kiss::Language>,
) -> Result<PlannedSelectors, String> {
    plan_selectors(mode, main, base, &[], &[], lang, None)
}

#[test]
fn row_g_untracked_rust_source_commit_only() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    ensure_main_branch(tmp.path());
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("src").join("lib.rs"),
        "pub fn value() -> u32 { 1 }\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "-A"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "base"])
            .status()
            .unwrap()
            .success()
    );
    checkout_branch(tmp.path(), "feature");
    let untracked = tmp.path().join("src").join("new_src.rs");
    fs::write(&untracked, "pub fn fresh() -> u32 { 1 }\n").unwrap();
    let commit = with_cwd(tmp.path(), || {
        plan(TestChangeMode::Commit, None, None, Some(kiss::Language::Rust))
    })
    .expect("commit plan");
    assert!(
        commit
            .rust_source_paths
            .iter()
            .any(|p| p.ends_with("new_src.rs")),
        "commit: untracked Rust source must appear in rust_source_paths, got {:?}",
        commit.rust_source_paths
    );
    let base = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Base,
            None,
            Some("main"),
            Some(kiss::Language::Rust),
        )
    })
    .expect("base plan");
    assert!(
        !base
            .rust_source_paths
            .iter()
            .any(|p| p.ends_with("new_src.rs")),
        "base: untracked Rust source must not appear, got {:?}",
        base.rust_source_paths
    );
    let main = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Main,
            Some("main"),
            None,
            Some(kiss::Language::Rust),
        )
    })
    .expect("main plan");
    assert!(
        !main
            .rust_source_paths
            .iter()
            .any(|p| p.ends_with("new_src.rs")),
        "main: untracked Rust source must not appear, got {:?}",
        main.rust_source_paths
    );
}

#[test]
fn row_h_base_and_main_use_snapshot_delta_not_historical_sources() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let (baseline, lib) = warm_base_demo_with_historical_source(&tmp);
    let base_planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Base,
            None,
            Some(&baseline),
            Some(kiss::Language::Rust),
        )
    })
    .expect("base plan selectors");
    let main_planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Main,
            Some(&baseline),
            None,
            Some(kiss::Language::Rust),
        )
    })
    .expect("main plan selectors");
    assert_base_delta_plan(&base_planned, &lib);
    assert_base_delta_plan(&main_planned, &lib);
}

#[test]
fn row_i_base_and_main_same_tree_yield_identical_selectors() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    let baseline = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    edit_rust_covered_source(&lib, 2);
    let base_planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Base,
            None,
            Some(&baseline),
            Some(kiss::Language::Rust),
        )
    })
    .expect("base same-tree");
    let main_planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Main,
            Some(&baseline),
            None,
            Some(kiss::Language::Rust),
        )
    })
    .expect("main same-tree");
    assert_eq!(
        base_planned.rs_sel, main_planned.rs_sel,
        "base≡main same-tree: rs_sel must match"
    );
    assert_eq!(
        base_planned.py_sel, main_planned.py_sel,
        "base≡main same-tree: py_sel must match"
    );
    assert_eq!(
        base_planned.rust_population_required, main_planned.rust_population_required,
        "base≡main same-tree: rust_population_required must match"
    );
    assert_eq!(
        base_planned.python_population_required, main_planned.python_population_required,
        "base≡main same-tree: python_population_required must match"
    );
    assert_eq!(base_planned.rs_sel, vec![RS_COVERING_SELECTOR.to_string()]);
}

fn assert_run_test_dry_run(mode: TestChangeMode, main: Option<&str>, base: Option<&str>) {
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    edit_rust_covered_source(&lib, 2);
    let planned = with_cwd(tmp.path(), || plan(mode, main, base, Some(kiss::Language::Rust)))
        .unwrap_or_else(|e| panic!("{mode:?} plan: {e}"));
    assert_eq!(
        planned.rs_sel,
        vec![RS_COVERING_SELECTOR.to_string()],
        "{mode:?}: dry-run covering selector"
    );
    let code = with_cwd(tmp.path(), || {
        run_test(RunTestCmdArgs {
            invocation: match mode {
                TestChangeMode::Commit => crate::bin_cli::args::TestInvocation::Commit,
                TestChangeMode::Base => crate::bin_cli::args::TestInvocation::Base,
                TestChangeMode::Main => crate::bin_cli::args::TestInvocation::Main,
            },
            main_branch_cli: main,
            base_branch_cli: base,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            ignore: &[],
            lang_filter: Some(kiss::Language::Rust),
            config_main_branch: None,
        })
    });
    assert_eq!(code, 0, "{mode:?}: run_test dry-run exit");
}

#[test]
fn row_j_commit_run_test_dry_run_exits_zero() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_run_test_dry_run(TestChangeMode::Commit, None, None);
}

#[test]
fn row_j_base_run_test_dry_run_exits_zero() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_run_test_dry_run(TestChangeMode::Base, None, Some("main"));
}

#[test]
fn row_j_main_run_test_dry_run_exits_zero() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_run_test_dry_run(TestChangeMode::Main, Some("main"), None);
}

#[test]
fn row_k_run_test_base_without_other_refs_fails() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git(&tmp);
    ensure_main_branch(tmp.path());
    fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "only"])
            .status()
            .unwrap()
            .success()
    );
    let plan_err = match with_cwd(tmp.path(), || {
        plan(TestChangeMode::Base, None, None, Some(kiss::Language::Rust))
    }) {
        Ok(_) => panic!("base: auto-detect must fail with one branch"),
        Err(err) => err,
    };
    assert!(
        plan_err.contains("--base-branch"),
        "base: plan error must mention --base-branch, got {plan_err}"
    );
    let code = with_cwd(tmp.path(), || {
        run_test(RunTestCmdArgs {
            invocation: crate::bin_cli::args::TestInvocation::Base,
            main_branch_cli: None,
            base_branch_cli: None,
            dry_run: true,
            force_rerun: false,
            metrics: false,
            jobs: 1,
            extra: &[],
            ignore: &[],
            lang_filter: Some(kiss::Language::Rust),
            config_main_branch: None,
        })
    });
    assert_ne!(code, 0, "base: run_test without other refs must be non-zero");
}
