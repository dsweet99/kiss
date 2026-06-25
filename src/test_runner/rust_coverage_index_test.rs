use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::*;

fn write_entry(
    repo_root: &Path,
    name: &str,
    selector: &str,
    status: TestStatus,
    coverage: RustLineCoverage,
) {
    let path = rust_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": CACHE_SCHEMA_VERSION,
        "selector": selector,
        "status": status,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

#[test]
fn rebuild_index_maps_covered_files_to_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let other = tmp.path().join("src").join("other.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    fs::write(&other, "pub fn other() {}\n").unwrap();
    write_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_entry(
        tmp.path(),
        "b",
        "test_other",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(other.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    let index = rebuild_rust_coverage_index(tmp.path()).unwrap();

    assert_eq!(
        index["src/lib.rs"],
        BTreeSet::from(["test_lib".to_string()])
    );
    assert_eq!(
        index["src/other.rs"],
        BTreeSet::from(["test_other".to_string()])
    );
    assert!(rust_coverage_index_path(tmp.path()).exists());
    assert_eq!(load_current_rust_coverage_index(tmp.path()).unwrap(), index);
}

#[test]
fn rebuild_index_ignores_unusable_entries_and_outside_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn lib() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("other.rs"), "pub fn other() {}\n").unwrap();

    write_failed_entry(tmp.path());
    write_empty_entry(tmp.path());
    write_outside_entry(tmp.path(), outside.path());
    write_malformed_entries(tmp.path());

    let index = rebuild_rust_coverage_index(tmp.path()).unwrap();

    assert!(index.is_empty());
}

fn write_failed_entry(repo_root: &Path) {
    write_entry(
        repo_root,
        "failed",
        "test_failed",
        TestStatus::Failed,
        RustLineCoverage {
            files: BTreeMap::from([(
                repo_root
                    .join("src")
                    .join("lib.rs")
                    .to_string_lossy()
                    .to_string(),
                BTreeSet::from([1]),
            )]),
        },
    );
}

fn write_empty_entry(repo_root: &Path) {
    write_entry(
        repo_root,
        "empty",
        "test_empty",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::new(),
        },
    );
}

fn write_outside_entry(repo_root: &Path, outside_root: &Path) {
    write_entry(
        repo_root,
        "outside",
        "test_outside",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                outside_root.join("other.rs").to_string_lossy().to_string(),
                BTreeSet::from([1]),
            )]),
        },
    );
}

fn write_malformed_entries(repo_root: &Path) {
    fs::write(
        rust_coverage_cache_root(repo_root)
            .join("entries")
            .join("bad.json"),
        "{not json",
    )
    .unwrap();
    fs::write(
        rust_coverage_cache_root(repo_root)
            .join("entries")
            .join("old.json"),
        serde_json::json!({
            "schema_version": "old",
            "selector": "test_old",
            "status": "Passed",
            "coverage": { "files": { "src/lib.rs": [1] } }
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn selectors_for_source_paths_requires_every_file_to_be_indexed() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let missing = tmp.path().join("src").join("missing.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    fs::write(&missing, "pub fn missing() {}\n").unwrap();
    let index = BTreeMap::from([(
        "src/lib.rs".to_string(),
        BTreeSet::from(["test_lib".to_string()]),
    )]);

    assert_eq!(
        selectors_for_source_paths(tmp.path(), std::slice::from_ref(&lib), &index).unwrap(),
        BTreeSet::from(["test_lib".to_string()])
    );
    assert!(
        selectors_for_source_paths(tmp.path(), &[lib, missing], &index).is_none(),
        "missing source files require population"
    );
}

#[test]
fn stale_index_is_not_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = rust_coverage_index_path(tmp.path());
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(
        index_path,
        serde_json::json!({
            "schema_version": INDEX_SCHEMA_VERSION,
            "source_root": normalized_repo_root(tmp.path()),
            "entries_fingerprint": "stale",
            "files": {}
        })
        .to_string(),
    )
    .unwrap();

    assert!(load_current_rust_coverage_index(tmp.path()).is_none());
}

#[test]
fn empty_sources_and_empty_cache_have_empty_selection_contracts() {
    let tmp = tempfile::tempdir().unwrap();

    let index = rebuild_rust_coverage_index(tmp.path()).unwrap();
    let selected = select_rust_source_selectors_from_index(tmp.path(), &[]).unwrap();

    assert!(index.is_empty());
    assert!(selected.is_empty());
    assert!(rust_coverage_cache_root(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache"));
    assert!(rust_coverage_index_path(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache/index.json"));
}

#[test]
fn bad_index_files_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = rust_coverage_index_path(tmp.path());
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(&index_path, "{not json").unwrap();
    assert!(load_current_rust_coverage_index(tmp.path()).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": "old",
            "source_root": normalized_repo_root(tmp.path()),
            "entries_fingerprint": entries_fingerprint(&rust_coverage_cache_root(tmp.path())).unwrap(),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path()).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": INDEX_SCHEMA_VERSION,
            "source_root": "/not/this/repo",
            "entries_fingerprint": entries_fingerprint(&rust_coverage_cache_root(tmp.path())).unwrap(),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path()).is_none());
}

#[test]
fn path_fingerprint_and_temp_file_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("other.rs");
    fs::write(&outside_file, "pub fn other() {}\n").unwrap();

    assert_eq!(
        repo_relative_coverage_file(tmp.path(), &lib.to_string_lossy()),
        Some("src/lib.rs".to_string())
    );
    assert_eq!(
        repo_relative_path(tmp.path(), Path::new("src/lib.rs")),
        Some("src/lib.rs".to_string())
    );
    assert!(repo_relative_path(tmp.path(), &outside_file).is_none());

    let first = entries_fingerprint(&rust_coverage_cache_root(tmp.path())).unwrap();
    write_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    let second = entries_fingerprint(&rust_coverage_cache_root(tmp.path())).unwrap();
    assert_ne!(first, second);

    let temp_path = tmp.path().join("created-once.tmp");
    let _file = create_new_file(&temp_path).unwrap();
    assert!(create_new_file(&temp_path).is_err());
    assert_ne!(unique_suffix(), "");
}
