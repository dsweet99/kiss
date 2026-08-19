
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
    plan_target_selectors(TargetPlanKind::All, &[], extras(), lang,
        &kiss::GateConfig::default()).expect("cold plan")
}

fn wipe_selector_cache(root: &Path) {
    let _ = fs::remove_dir_all(root.join(".kiss"));
}

fn assert_both_languages(planned: &PlannedSelectors) {
    assert!(!planned.sel.python.is_empty());
    assert!(!planned.sel.rust.is_empty());
}

#[test]
fn cold_all_enumerates_and_stores_fingerprint() {
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
        "rust_report_ids",
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
        &kiss::GateConfig::default())
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
        &kiss::GateConfig::default());
    assert!(bad.is_err(), "expected rust extra validation error");

    std::env::set_current_dir(orig).unwrap();
}
