use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rslip::LineCoverage;

use crate::test_runner::python_coverage_index::{
    PYTHON_SELECTOR_DISCOVERY_VERSION, PythonPopulationManifestIdentity,
    build_python_coverage_index, create_new_python_file, is_kiss_rslip_cache_dir,
    load_python_entries_for_line_selection, load_python_entry_for_index,
    normalized_python_repo_root, python_changed_line_rels, python_coverage_cache_root,
    python_coverage_entry_paths, python_entries_fingerprint, python_fnv1a64,
    python_population_manifest_is_current_with_identity, python_repo_relative_coverage_file,
    python_repo_relative_path, python_selectors_by_changed_file_line,
    python_selectors_for_source_paths, python_unique_suffix, read_python_population_manifest,
    write_python_population_manifest_with_identity,
};

fn write_entry(repo_root: &Path, name: &str, selector: &str, coverage: LineCoverage) {
    let path = python_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": rslip::CACHE_SCHEMA_VERSION,
        "nodeid": selector,
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    std::fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

#[test]
fn python_coverage_index_witnesses_manifest_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    let identity = PythonPopulationManifestIdentity {
        cache_schema_version: rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version: "3.12.0".to_string(),
        pytest_version: "8.0.0".to_string(),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
    };

    let has_versions = identity.has_python_tool_versions();
    assert!(has_versions);
    let missing_manifest = read_python_population_manifest(tmp.path());
    assert!(missing_manifest.is_none());
    write_python_population_manifest_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    )
    .unwrap();
    let manifest = read_python_population_manifest(tmp.path()).unwrap();
    let root = normalized_python_repo_root(tmp.path());
    let identity_matches = manifest.matches_python_identity(&identity, &root);
    let selectors_match = manifest.matches_python_selectors(std::slice::from_ref(&selector));
    let manifest_is_current = python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    );
    assert!(identity_matches);
    assert!(selectors_match);
    assert!(manifest_is_current);
}

#[test]
fn python_coverage_index_witnesses_storage_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_entry(
        tmp.path(),
        "a",
        &selector,
        LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    let entry_path = python_coverage_cache_root(tmp.path())
        .join("entries")
        .join("a.json");
    let loaded_entry = load_python_entry_for_index(&entry_path).unwrap();
    assert_eq!(loaded_entry.0, selector);
    let entry_paths = python_coverage_entry_paths(&python_coverage_cache_root(tmp.path()));
    assert_eq!(entry_paths.len(), 1);
    let entries_fp = python_entries_fingerprint(&python_coverage_cache_root(tmp.path()));
    assert!(entries_fp.is_ok());
    let is_cache_dir = is_kiss_rslip_cache_dir(&tmp.path().join(".kiss").join("rslip_cache"));
    assert!(is_cache_dir);
    let rel_coverage = python_repo_relative_coverage_file(tmp.path(), &app.to_string_lossy());
    assert_eq!(rel_coverage, Some("app.py".to_string()));
    let outside_rel = python_repo_relative_path(tmp.path(), Path::new("/outside.py"));
    assert!(outside_rel.is_none());
    let suffix = python_unique_suffix();
    assert_ne!(suffix, "");
    let hash_a = python_fnv1a64(0xcbf2_9ce4_8422_2325, b"a");
    let hash_b = python_fnv1a64(0xcbf2_9ce4_8422_2325, b"b");
    assert_ne!(hash_a, hash_b);
    let created = tmp.path().join("created.txt");
    create_new_python_file(&created).unwrap();
    let duplicate_create = create_new_python_file(&created);
    assert!(duplicate_create.is_err());
}

#[test]
fn python_coverage_index_witnesses_selection_helpers() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    let empty = tmp.path().join("empty.py");
    std::fs::write(&app, "def value():\n    return 1\n").unwrap();
    std::fs::write(&empty, "VALUE = 0\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_entry(
        tmp.path(),
        "a",
        &selector,
        LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );

    let index = build_python_coverage_index(tmp.path());
    let source_selectors =
        python_selectors_for_source_paths(tmp.path(), &[app.clone(), empty], &index).unwrap();
    assert_eq!(source_selectors, BTreeSet::from([selector.clone()]));
    let line_rels = python_changed_line_rels(
        tmp.path(),
        &BTreeMap::from([(app.clone(), BTreeSet::from([1]))]),
    );
    assert_eq!(
        line_rels,
        BTreeMap::from([("app.py".to_string(), BTreeSet::from([1]))])
    );
    let line_selectors = python_selectors_by_changed_file_line(
        tmp.path(),
        &BTreeMap::from([("app.py".to_string(), BTreeSet::from([1]))]),
    );
    assert_eq!(
        line_selectors,
        BTreeMap::from([("app.py".to_string(), BTreeSet::from([selector]))])
    );
    let line_entries =
        load_python_entries_for_line_selection(&python_coverage_cache_root(tmp.path()));
    assert_eq!(line_entries.len(), 1);
}
