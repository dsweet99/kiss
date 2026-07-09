use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::test_support::write_test_entry;
use super::*;

#[test]
fn population_manifest_requires_matching_selectors_and_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["other_test".to_string()],
        &[],
    ));

    fs::write(&lib, "pub fn lib() {}\npub fn changed() {}\n").unwrap();
    assert!(
        !rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "source changes invalidate population freshness"
    );

    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    write_test_entry(
        tmp.path(),
        "b",
        "test_other",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    assert!(
        rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "cache-only entry changes do not invalidate population freshness"
    );
    assert!(
        !rust_population_manifest_is_current_for_args(
            tmp.path(),
            &["test_lib".to_string(), "test_other".to_string()],
            &[],
        ),
        "selector universe changes invalidate population freshness"
    );
}
