use super::population_repair::repair_stale_population_on_all_mode_accept;
use crate::test_runner::lang_iface::{AcceptMode, EnsureRequest};
use crate::test_runner::rust_coverage_index::{
    rust_population_manifest_is_current_for_args, rust_population_manifest_path,
    write_rust_population_manifest_for_args, write_test_entry,
};
use kiss::rpytest_runner::TestStatus;
use kiss::rust_llvm_cov_runner::RustLineCoverage;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

fn all_mode_request(repo: &std::path::Path) -> EnsureRequest {
    EnsureRequest {
        repo_root: repo.to_path_buf(),
        mode: AcceptMode::All,
        lang_filter: Some(kiss::Language::Rust),
        ignore: vec![],
        force: false,
        force_selectors: Vec::new(),
        jobs: 1,
        gate: kiss::GateConfig::default(),
        extras: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec![],
        },
        planned: crate::test_runner::language_keyed::LanguageKeyed {
            python: vec![],
            rust: vec!["test_lib".into()],
        },
    }
}

fn write_demo_crate(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = root.join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    write_test_entry(
        root,
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_rust_population_manifest_for_args(root, &["test_lib".to_string()], &[]).unwrap();
}

#[test]
fn empty_all_mode_plan_skips_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let req = all_mode_request(tmp.path());
    assert!(!repair_stale_population_on_all_mode_accept(&req, &[]));
}

#[test]
fn current_all_mode_manifest_skips_repair() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate(tmp.path());
    let req = all_mode_request(tmp.path());
    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    assert!(!repair_stale_population_on_all_mode_accept(
        &req,
        &["test_lib".to_string()],
    ));
}

#[test]
fn broken_all_mode_manifest_is_rebuilt() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate(tmp.path());
    let path = rust_population_manifest_path(tmp.path());
    fs::write(&path, "{ broken").unwrap();
    let req = all_mode_request(tmp.path());
    assert!(repair_stale_population_on_all_mode_accept(
        &req,
        &["test_lib".to_string()],
    ));
    serde_json::from_str::<serde_json::Value>(&fs::read_to_string(path).unwrap()).unwrap();
}
