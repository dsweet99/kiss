use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rslip::LineCoverage;

fn identity() -> PythonPopulationManifestIdentity {
    PythonPopulationManifestIdentity {
        cache_schema_version: rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version: "3.12.0".to_string(),
        pytest_version: "8.0.0".to_string(),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
    }
}

fn write_entry(repo_root: &Path, name: &str, selector: &str, coverage: LineCoverage) {
    let path = python_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": rslip::CACHE_SCHEMA_VERSION,
        "nodeid": selector,
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}

#[test]
fn manifest_and_storage_helpers_are_referenced_from_external_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    fs::write(&app, "def value():\n    return 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_entry(
        tmp.path(),
        "a",
        &selector,
        LineCoverage {
            files: BTreeMap::from([
                (app.to_string_lossy().to_string(), BTreeSet::from([1])),
                (
                    "<frozen importlib._bootstrap>".to_string(),
                    BTreeSet::from([1]),
                ),
                (
                    ".kiss/rslip_cache/rslip_runtime.py".to_string(),
                    BTreeSet::from([1]),
                ),
                ("/outside.py".to_string(), BTreeSet::from([1])),
            ]),
        },
    );
    let mut identity = identity();
    assert!(identity.has_python_tool_versions());
    assert!(read_python_population_manifest(tmp.path()).is_none());
    write_python_population_manifest_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    )
    .unwrap();
    let manifest = read_python_population_manifest(tmp.path()).unwrap();
    assert!(manifest.matches_python_identity(&identity, &normalized_python_repo_root(tmp.path())));
    assert!(manifest.matches_python_selectors(std::slice::from_ref(&selector)));
    assert!(python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity
    ));
    identity.pytest_version.clear();
    assert!(!identity.has_python_tool_versions());
    assert!(!manifest.matches_python_identity(&identity, &normalized_python_repo_root(tmp.path())));
    assert!(!python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity
    ));
}

#[test]
fn selector_path_and_hash_helpers_are_referenced_from_external_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    let other = tmp.path().join("other.py");
    fs::write(&app, "def value():\n    return 1\n").unwrap();
    fs::write(&other, "VALUE = 2\n").unwrap();
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
    assert_eq!(
        load_python_entry_for_index(&entry_path).unwrap().0,
        selector
    );
    let index = build_python_coverage_index(tmp.path());
    assert_eq!(
        index.keys().cloned().collect::<Vec<_>>(),
        vec!["app.py".to_string()]
    );
    assert_eq!(
        python_selectors_for_source_paths(tmp.path(), &[app.clone(), other], &index).unwrap(),
        BTreeSet::from(["tests/test_app.py::test_value".to_string()])
    );
    assert_eq!(
        python_changed_line_rels(
            tmp.path(),
            &BTreeMap::from([(app.clone(), BTreeSet::from([1]))])
        ),
        BTreeMap::from([("app.py".to_string(), BTreeSet::from([1]))])
    );
    assert_eq!(
        python_selectors_by_changed_file_line(
            tmp.path(),
            &BTreeMap::from([("app.py".to_string(), BTreeSet::from([1]))])
        ),
        BTreeMap::from([(
            "app.py".to_string(),
            BTreeSet::from(["tests/test_app.py::test_value".to_string()])
        )])
    );
    assert_eq!(
        load_python_entries_for_line_selection(&python_coverage_cache_root(tmp.path())).len(),
        1
    );
    assert_ne!(
        python_entries_fingerprint(&python_coverage_cache_root(tmp.path())).unwrap(),
        ""
    );
    assert!(is_kiss_rslip_cache_dir(
        &tmp.path().join(".kiss").join("rslip_cache")
    ));
    assert_eq!(
        python_repo_relative_coverage_file(tmp.path(), &app.to_string_lossy()),
        Some("app.py".to_string())
    );
    assert!(python_repo_relative_path(tmp.path(), Path::new("/outside.py")).is_none());
    assert_eq!(
        normalized_python_repo_root(tmp.path()),
        tmp.path().canonicalize().unwrap().display().to_string()
    );
    let created = tmp.path().join("created.txt");
    create_new_python_file(&created).unwrap();
    assert!(create_new_python_file(&created).is_err());
    assert_ne!(python_unique_suffix(), "");
    assert_ne!(
        python_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
        python_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
    );
}
