use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::test_support::write_test_entry;
use super::*;

#[test]
fn rebuild_index_maps_covered_files_to_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let other = tmp.path().join("src").join("other.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    fs::write(&other, "pub fn other() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_test_entry(
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
    write_test_entry(
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
    write_test_entry(
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
    write_test_entry(
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
fn selectors_for_source_paths_skips_uncovered_siblings() {
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
    assert_eq!(
        selectors_for_source_paths(tmp.path(), &[lib, missing], &index).unwrap(),
        BTreeSet::from(["test_lib".to_string()]),
        "uncovered source files contribute no selectors but do not abort siblings"
    );
}

#[test]
fn selectors_for_changed_lines_use_line_intersection() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn a() {}\npub fn b() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_line_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "b",
        "test_line_2",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([2]))]),
        },
    );

    let selected = select_rust_source_selectors_for_changed_lines(
        tmp.path(),
        &BTreeMap::from([(lib, BTreeSet::from([2]))]),
    )
    .unwrap();

    assert_eq!(selected, BTreeSet::from(["test_line_2".to_string()]));
}

#[test]
fn selectors_for_changed_lines_require_every_changed_file_to_match() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn a() {}\npub fn b() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_line_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    assert!(
        select_rust_source_selectors_for_changed_lines(
            tmp.path(),
            &BTreeMap::from([(lib, BTreeSet::from([2]))]),
        )
        .is_none(),
        "missing changed-line coverage falls back to file-level selection"
    );
}

#[test]
fn hybrid_selection_falls_back_per_file() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let precise = tmp.path().join("src").join("precise.rs");
    let fallback = tmp.path().join("src").join("fallback.rs");
    fs::write(&precise, "pub fn a() {}\npub fn b() {}\n").unwrap();
    fs::write(&fallback, "pub fn c() {}\npub fn d() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "precise_line_1",
        "test_precise_1",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(precise.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "precise_line_2",
        "test_precise_2",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(precise.to_string_lossy().to_string(), BTreeSet::from([2]))]),
        },
    );
    write_test_entry(
        tmp.path(),
        "fallback_file",
        "test_fallback_file",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(fallback.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();

    let selected = select_rust_source_selectors_hybrid(
        tmp.path(),
        &[precise.clone(), fallback.clone()],
        &BTreeMap::from([
            (precise, BTreeSet::from([2])),
            (fallback, BTreeSet::from([99])),
        ]),
    )
    .unwrap();

    assert_eq!(
        selected,
        BTreeSet::from([
            "test_precise_2".to_string(),
            "test_fallback_file".to_string()
        ])
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
fn population_manifest_requires_matching_selectors_inputs_and_entries() {
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
        !rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "cache entry changes invalidate the manifest"
    );
}
