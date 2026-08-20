use super::warm_cov_caches_after_tests;
use crate::test_runner::python_coverage_index::{
    write_python_coverage_snapshot, write_python_population_manifest_for_args,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn write_src(repo: &Path) {
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("a.py"), "x = 1\n").unwrap();
}

#[test]
fn warm_cov_caches_after_tests_is_callable_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_src(repo);
    let gate = kiss::GateConfig::default();

    warm_cov_caches_after_tests(repo, Some(kiss::Language::Python), &[], &gate, &[]);
    assert!(!repo.join(".kiss/cov_records_cache.json").exists());
}

#[test]
fn warm_cov_caches_after_tests_writes_records_when_snapshot_present() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::env::set_current_dir(repo).unwrap();
    write_src(repo);
    fs::write(
        repo.join(".kissconfig"),
        "[test]\ntest_coverage_threshold = 75\n",
    )
    .unwrap();

    let selector = "tests/test_a.py::test_x".to_string();
    write_python_population_manifest_for_args(repo, std::slice::from_ref(&selector), &[]).unwrap();
    let mut covered = BTreeMap::new();
    covered.insert("a.py".to_string(), BTreeSet::from([1u32]));
    write_python_coverage_snapshot(repo, &covered).unwrap();

    let gate = kiss::GateConfig::load();
    warm_cov_caches_after_tests(repo, Some(kiss::Language::Python), &[], &gate, &[]);
    assert!(
        repo.join(".kiss/cov_records_cache.json").is_file(),
        "expected records cache after successful warm"
    );
    assert!(
        repo.join(".kiss/cov_file_list_cache.json").is_file(),
        "expected file-list cache after successful warm"
    );

    let before = fs::metadata(repo.join(".kiss/cov_records_cache.json"))
        .unwrap()
        .modified()
        .unwrap();
    warm_cov_caches_after_tests(repo, Some(kiss::Language::Python), &[], &gate, &[]);
    let after = fs::metadata(repo.join(".kiss/cov_records_cache.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
}
