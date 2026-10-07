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
fn stage_names_use_kiss_test_stage_format() {
    for name in [
        "python_generation_publish",
        "python_source_fingerprint",
        "rust_identity",
        "planning_select",
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
fn python_all_plan_keeps_provided_universe_and_requires_population_without_records() {
    let tmp = TempDir::new().unwrap();
    init_git_repo(tmp.path());
    let provided: Vec<String> = vec![
        "tests/test_a.py::test_a".into(),
        "tests/test_b.py::test_b".into(),
    ];
    let plan = |selectors: Vec<String>| {
        crate::test_runner::lang_iface::records::records_all_mode_plan(
            tmp.path(),
            "python",
            selectors,
        )
    };
    let full = plan(provided.clone());
    assert_eq!(full.planned, provided);
    assert!(full.population_required);
    let empty = plan(Vec::new());
    assert!(empty.planned.is_empty());
    assert!(!empty.population_required);
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
        "All must keep b_ok, not only the commit-selecting test: {:?}",
        planned.sel.rust
    );
}

#[test]
fn all_mode_rust_edit_keeps_universe_and_reruns_every_test() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    crate::test_runner::test_mode_fixtures::init_git(&tmp);
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='all_ordinary'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn a() -> u8 { 1 }\npub fn b() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_ok() { assert_eq!(super::a(), 1); }\n    #[test]\n    fn b_ok() { assert_eq!(super::b(), 2); }\n}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("src/extra_test.rs"),
        "#[test]\nfn only_extra() { assert!(true); }\n",
    )
    .unwrap();
    let mut lib = fs::read_to_string(tmp.path().join("src/lib.rs")).unwrap();
    lib.push_str("#[cfg(test)]\nmod extra_test;\n");
    fs::write(tmp.path().join("src/lib.rs"), lib).unwrap();
    let selectors = ["only_extra", "tests::a_ok", "tests::b_ok"];
    crate::test_runner::lang_rust::nextest::generate_lockfile(tmp.path());
    crate::test_runner::lang_rust::nextest::store_records(
        tmp.path(),
        &selectors.map(|s| (s, kiss::rpytest_runner::TestStatus::Passed)),
    );
    let selector_list: Vec<String> = selectors.iter().map(|s| s.to_string()).collect();
    let witness = crate::test_runner::lang_rust::try_load_rust_execution_witness(tmp.path()).ok();
    assert!(
        crate::test_runner::lang_iface::records::record_misses(&selector_list, witness.as_ref())
            .is_empty(),
        "fresh records must hold before the edit"
    );
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
        "pub fn a() -> u8 { 3 }\npub fn b() -> u8 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn a_ok() { assert_eq!(super::a(), 3); }\n    #[test]\n    fn b_ok() { assert_eq!(super::b(), 2); }\n}\n#[cfg(test)]\nmod extra_test;\n",
    )
    .unwrap();
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned = plan_all(Some(Language::Rust));
    std::env::set_current_dir(orig).unwrap();
    assert!(
        planned.sel.rust.iter().any(|s| s.contains("a_ok")),
        "All must keep a_ok: {:?}",
        planned.sel.rust
    );
    assert!(
        planned.sel.rust.iter().any(|s| s.contains("b_ok")),
        "All must keep b_ok: {:?}",
        planned.sel.rust
    );
    assert!(
        planned.sel.rust.iter().any(|s| s == "only_extra"),
        "All must keep only_extra: {:?}",
        planned.sel.rust
    );
    assert!(
        !planned.population_required.rust,
        "ordinary lib.rs edit must not require a full Rust population"
    );
    let witness = crate::test_runner::lang_rust::try_load_rust_execution_witness(tmp.path()).ok();
    let misses =
        crate::test_runner::lang_iface::records::record_misses(&planned.sel.rust, witness.as_ref());
    assert_eq!(
        misses.len(),
        planned.sel.rust.len(),
        "a Rust source edit must rerun every Rust test: {misses:?}"
    );
}
