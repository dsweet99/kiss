use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use kiss::rpytest_runner::TestStatus;
use kiss::rslip::LineCoverage;

fn identity() -> PythonPopulationManifestIdentity {
    PythonPopulationManifestIdentity {
        cache_schema_version: kiss::rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version: "3.12.0".to_string(),
        pytest_version: "8.0.0".to_string(),
        pytest_args: Vec::new(),
        env: BTreeMap::new(),
    }
}

fn write_entry(
    repo_root: &Path,
    _name: &str,
    selector: &str,
    status: TestStatus,
    coverage: LineCoverage,
) -> std::path::PathBuf {
    super::storage::write_python_record_fixture(repo_root, selector, status, coverage)
}

fn write_passed_entry(
    repo_root: &Path,
    name: &str,
    selector: &str,
    coverage: LineCoverage,
) -> std::path::PathBuf {
    write_entry(repo_root, name, selector, TestStatus::Passed, coverage)
}

#[test]
fn manifest_and_storage_helpers_are_referenced_from_external_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    fs::write(&app, "def value():\n    return 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_passed_entry(
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
