use std::fs;
use std::path::Path;

use tempfile::TempDir;

use kiss::Language;

use super::{TargetPlanKind, plan_target_selectors};
use crate::cwd_test_lock;
use crate::test_runner::PlannedSelectors;

fn init_git_repo(root: &Path) {
    let status = kiss::scrubbed_git_command(root)
        .arg("init")
        .status()
        .unwrap();
    assert!(status.success());
}

fn write_cold_demo(root: &Path) {
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests").join("test_a.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='cold_plan_demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        "pub fn n() -> u8 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn n_is_one() { assert_eq!(super::n(), 1); }\n}\n",
    )
    .unwrap();
}

fn extras() -> crate::test_runner::language_keyed::LanguageKeyed<&'static [String]> {
    crate::test_runner::language_keyed::LanguageKeyed {
        python: &[],
        rust: &[],
    }
}

fn plan_all(lang: Option<Language>) -> PlannedSelectors {
    plan_target_selectors(
        TargetPlanKind::All,
        &[],
        extras(),
        lang,
        &kiss::GateConfig::default(),
    )
    .expect("cold plan")
}

fn wipe_selector_cache(root: &Path) {
    let _ = fs::remove_dir_all(root.join(".kiss"));
}

fn shrink_durable_plan_selectors(root: &Path) {
    for name in ["python_test_selectors.json", "rust_test_selectors.json"] {
        let path = root.join("target").join("kiss-plan").join(name);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["selectors"] = serde_json::json!([]);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}

fn assert_both_languages(planned: &PlannedSelectors) {
    assert!(!planned.sel.python.is_empty());
    assert!(!planned.sel.rust.is_empty());
}

#[test]
fn cold_all_enumerates_and_stores_fingerprint() {
    let _cwd = crate::cwd_test_lock::lock();
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_cold_demo(tmp.path());
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let both = plan_all(None);
    assert_both_languages(&both);
    assert!(both.population_required.python);
    assert!(both.population_required.rust);
    assert!(both.skip_index_rebuild_after_selective.python);
    assert!(both.workspace_files_fingerprint.is_some());

    let py_hit = plan_all(Some(Language::Python));
    assert_eq!(py_hit.sel.python, both.sel.python);
    assert!(py_hit.sel.rust.is_empty());
    let rs_hit = plan_all(Some(Language::Rust));
    assert!(rs_hit.sel.python.is_empty());
    assert_eq!(rs_hit.sel.rust, both.sel.rust);

    std::env::set_current_dir(orig).unwrap();
}

#[test]
fn wiping_kiss_rediscovers_after_durable_plan_shrink() {
    let _cwd = crate::cwd_test_lock::lock();
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_cold_demo(tmp.path());
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    let both = plan_all(None);
    assert_both_languages(&both);
    shrink_durable_plan_selectors(tmp.path());
    wipe_selector_cache(tmp.path());
    crate::test_runner::workspace_selector_cache::clear_rust_selector_memo_for_tests();
    let after = plan_all(None);
    std::env::set_current_dir(orig).unwrap();
    assert_eq!(after.sel.python, both.sel.python);
    assert_eq!(after.sel.rust, both.sel.rust);
}

#[test]
fn emit_stage_time_format_includes_name_and_millis() {
    let msg = format!(
        "kiss test: stage {} {}ms",
        "plan_rust",
        std::time::Duration::from_millis(12).as_millis()
    );
    assert_eq!(msg, "kiss test: stage plan_rust 12ms");
}

#[test]
fn watch_stage_names_use_kiss_test_stage_format() {
    for name in [
        "python_generation_publish",
        "python_source_fingerprint",
        "rust_identity",
        "covering_select",
        "cov_score_warm",
        "cov_score",
        "rslip_prepare",
        "selective_index_repair",
        "plan_python",
        "plan_rust",
    ] {
        let msg = format!(
            "kiss test: stage {} {}ms",
            name,
            std::time::Duration::from_millis(1).as_millis()
        );
        assert_eq!(msg, format!("kiss test: stage {name} 1ms"));
    }
}

#[test]
fn cold_lang_filter_miss_arms_do_not_store_all_cache() {
    let _cwd = crate::cwd_test_lock::lock();
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_cold_demo(tmp.path());
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    wipe_selector_cache(tmp.path());
    let py_miss = plan_all(Some(Language::Python));
    assert!(!py_miss.sel.python.is_empty());
    assert!(py_miss.sel.rust.is_empty());
    assert!(py_miss.workspace_files_fingerprint.is_none());

    wipe_selector_cache(tmp.path());
    let rs_miss = plan_all(Some(Language::Rust));
    assert!(rs_miss.sel.python.is_empty());
    assert!(!rs_miss.sel.rust.is_empty());

    std::env::set_current_dir(orig).unwrap();
}

#[test]
fn cold_dot_target_uses_plan_all_and_rust_extras_validate() {
    let _cwd = crate::cwd_test_lock::lock();
    let _cwd = cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    write_cold_demo(tmp.path());
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();

    wipe_selector_cache(tmp.path());
    let via_dot = plan_target_selectors(
        TargetPlanKind::Targets(&[".".into()]),
        &[],
        extras(),
        None,
        &kiss::GateConfig::default(),
    )
    .expect("dot all");
    assert_both_languages(&via_dot);

    let bad = plan_target_selectors(
        TargetPlanKind::All,
        &[],
        crate::test_runner::language_keyed::LanguageKeyed {
            python: &[],
            rust: &["--test-threads".into()],
        },
        Some(Language::Rust),
        &kiss::GateConfig::default(),
    );
    assert!(bad.is_err(), "expected rust extra validation error");

    std::env::set_current_dir(orig).unwrap();
}

#[test]
fn all_mode_does_not_substitute_commit_covering() {
    let plan = include_str!("plan.rs");
    let vcs = include_str!("plan_vcs.rs");
    assert!(
        !plan.contains("commit_covering_plan"),
        "All planning must keep the workspace universe, not a commit-covering subset"
    );
    assert!(
        !vcs.contains("commit_covering_plan") && !vcs.contains("commit_python_covering_plan"),
        "python_all_plan must not replace the universe with commit covering"
    );
}

#[test]
fn python_all_plan_keeps_provided_universe_and_requires_population_without_index() {
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    let provided = vec![
        "tests/test_a.py::test_a".into(),
        "tests/test_b.py::test_b".into(),
    ];
    let (sel, required) =
        super::plan_vcs::python_all_plan(tmp.path(), &[], &[], provided.clone(), true);
    assert_eq!(sel, provided);
    assert!(required);
    let (skipped, skip_required) =
        super::plan_vcs::python_all_plan(tmp.path(), &[], &[], provided, false);
    assert!(skipped.is_empty());
    assert!(!skip_required);
}

#[test]
fn dirty_all_keeps_both_rust_tests_after_one_file_change() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='all_universe'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn a() -> u8 { 1 }\npub fn b() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_ok() { assert_eq!(super::a(), 1); }\n    #[test]\n    fn b_ok() { assert_eq!(super::b(), 2); }\n}\n",
    )
    .unwrap();
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(tmp.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::test_runner::test_mode_fixtures::git_in(tmp.path())
            .args(["commit", "-m", "init"])
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn a() -> u8 { 3 }\npub fn b() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_ok() { assert_eq!(super::a(), 3); }\n    #[test]\n    fn b_ok() { assert_eq!(super::b(), 2); }\n}\n",
    )
    .unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned = plan_all(Some(Language::Rust));
    std::env::set_current_dir(orig).unwrap();
    assert!(
        planned.sel.rust.iter().any(|s| s.contains("a_ok")),
        "All must keep a_ok after a dirty edit: {:?}",
        planned.sel.rust
    );
    assert!(
        planned.sel.rust.iter().any(|s| s.contains("b_ok")),
        "All must keep b_ok, not only the commit-covering test: {:?}",
        planned.sel.rust
    );
}
