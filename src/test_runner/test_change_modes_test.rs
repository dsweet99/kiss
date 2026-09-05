use tempfile::TempDir;

use crate::test_git::TestChangeMode;
use crate::test_runner::test_mode_fixtures::{
    PY_COVERING_SELECTOR, RS_COVERING_SELECTOR, edit_python_covered_source,
    edit_rust_covered_source, git_in, rewrite_python_population_after_edit,
    warm_committed_rust_demo, warm_multi_branch_rust_demo, warm_python_covering_demo, with_cwd,
};
use crate::test_runner::{PlannedSelectors, SelectorRunOptions, plan_selectors, run_selectors};

fn plan(
    mode: TestChangeMode,
    main: Option<&str>,
    base: Option<&str>,
    ignore: &[String],
    lang: Option<kiss::Language>,
) -> Result<PlannedSelectors, String> {
    plan_selectors(crate::test_runner::PlanSelectorsRequest {
        mode,
        main_branch_cli: main,
        base_branch_cli: base,
        ignore,
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &[],
        },
        lang_filter: lang,
        config_main_branch: None,
    })
}

fn mode_label(mode: TestChangeMode) -> &'static str {
    match mode {
        TestChangeMode::Commit => "commit",
        TestChangeMode::Base => "base",
        TestChangeMode::Main => "main",
    }
}

fn mode_plan_args(mode: TestChangeMode) -> (Option<&'static str>, Option<&'static str>) {
    match mode {
        TestChangeMode::Commit => (None, None),
        TestChangeMode::Base => (None, Some("main")),
        TestChangeMode::Main => (Some("main"), None),
    }
}

fn assert_rust_covering(mode: TestChangeMode) {
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    edit_rust_covered_source(&lib, 2);
    let (main, base) = mode_plan_args(mode);
    let planned = with_cwd(tmp.path(), || {
        plan(mode, main, base, &[], Some(kiss::Language::Rust))
    })
    .unwrap_or_else(|e| panic!("{} plan failed: {e}", mode_label(mode)));
    assert!(
        !planned.population_required.rust,
        "{}: rust_population_required must be false",
        mode_label(mode)
    );
    assert_eq!(
        planned.sel.rust,
        vec![RS_COVERING_SELECTOR.to_string()],
        "{}: covering Rust selector contract",
        mode_label(mode)
    );
}

fn assert_python_covering(mode: TestChangeMode) {
    let tmp = TempDir::new().unwrap();
    let app = warm_python_covering_demo(&tmp);
    edit_python_covered_source(&app, 2);
    rewrite_python_population_after_edit(tmp.path());
    let (main, base) = mode_plan_args(mode);
    let planned = with_cwd(tmp.path(), || {
        plan(mode, main, base, &[], Some(kiss::Language::Python))
    })
    .unwrap_or_else(|e| panic!("{} python plan failed: {e}", mode_label(mode)));
    assert!(
        !planned.population_required.python,
        "{}: python_population_required must be false",
        mode_label(mode)
    );
    assert_eq!(
        planned.sel.python,
        vec![PY_COVERING_SELECTOR.to_string()],
        "{}: covering Python selector contract",
        mode_label(mode)
    );
}

#[test]
fn row_a_commit_warm_rust_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_rust_covering(TestChangeMode::Commit);
}

#[test]
fn row_a_base_warm_rust_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_rust_covering(TestChangeMode::Base);
}

#[test]
fn row_a_main_warm_rust_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_rust_covering(TestChangeMode::Main);
}

#[test]
fn row_b_rust_test_file_only_selects_that_test() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_multi_branch_rust_demo(&tmp);
    let test_file = tmp.path().join("src").join("extra_test.rs");
    std::fs::write(&test_file, "#[test]\nfn only_extra() {}\n").unwrap();
    let mut lib_src = std::fs::read_to_string(&lib).unwrap();
    lib_src.push_str("#[cfg(test)]\nmod extra_test;\n");
    std::fs::write(&lib, lib_src).unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "src/extra_test.rs", "src/lib.rs"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "add-extra-test"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(&test_file, "#[test]\nfn only_extra() { assert!(true); }\n").unwrap();
    let planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Commit,
            None,
            None,
            &[],
            Some(kiss::Language::Rust),
        )
    })
    .expect("commit plan");
    assert!(
        planned.sel.rust.iter().any(|s| s == "only_extra"),
        "commit: changed Rust test file must select only_extra, got {:?}",
        planned.sel.rust
    );
}

#[test]
fn row_c_commit_warm_python_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_python_covering(TestChangeMode::Commit);
}

#[test]
fn row_c_base_warm_python_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_python_covering(TestChangeMode::Base);
}

#[test]
fn row_c_main_warm_python_edit_selects_covering() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    assert_python_covering(TestChangeMode::Main);
}

#[test]
fn row_d_empty_diff_dry_run_exits_zero() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let _ = warm_multi_branch_rust_demo(&tmp);
    let planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Commit,
            None,
            None,
            &[],
            Some(kiss::Language::Rust),
        )
    })
    .expect("empty commit plan");
    assert!(
        planned.sel.rust.is_empty() && planned.sel.python.is_empty(),
        "commit: empty diff must yield empty selectors"
    );
    let code = with_cwd(tmp.path(), || {
        run_selectors(
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
                plan_duration: std::time::Duration::ZERO,
                gate: kiss::GateConfig::default(),
            },
        )
    })
    .unwrap();
    assert_eq!(code, 0, "commit: empty dry-run exit");
}

#[test]
fn row_e_lang_rust_excludes_python_selectors() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let app = warm_python_covering_demo(&tmp);
    edit_python_covered_source(&app, 2);
    rewrite_python_population_after_edit(tmp.path());
    let rust_only = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Commit,
            None,
            None,
            &[],
            Some(kiss::Language::Rust),
        )
    })
    .expect("rust-only over python edit");
    assert!(
        rust_only.sel.python.is_empty() && rust_only.sel.rust.is_empty(),
        "commit --lang rust: python edit must not select python/rust, got py={:?} rs={:?}",
        rust_only.sel.python,
        rust_only.sel.rust
    );
}

#[test]
fn row_e_lang_python_excludes_rust_selectors() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    edit_rust_covered_source(&lib, 2);
    let py_only = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Commit,
            None,
            None,
            &[],
            Some(kiss::Language::Python),
        )
    })
    .expect("python-only over rust edit");
    assert!(
        py_only.sel.rust.is_empty() && py_only.sel.python.is_empty(),
        "commit --lang python: rust edit must not select rust/python, got py={:?} rs={:?}",
        py_only.sel.python,
        py_only.sel.rust
    );
}

#[test]
fn row_f_ignore_prefix_skips_edited_file() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_rust_demo(&tmp);
    edit_rust_covered_source(&lib, 2);
    let ignore = vec!["src".to_string()];
    let planned = with_cwd(tmp.path(), || {
        plan(
            TestChangeMode::Commit,
            None,
            None,
            &ignore,
            Some(kiss::Language::Rust),
        )
    })
    .expect("ignored plan");
    assert!(
        planned.sel.rust.is_empty(),
        "commit --ignore src: edited file must not drive selection, got {:?}",
        planned.sel.rust
    );
}
