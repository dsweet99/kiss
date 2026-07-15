use super::*;
use crate::test_runner::TestEnvVarGuard;
use crate::test_runner::python_coverage_index::storage::{
    normalized_python_repo_root, python_coverage_cache_root,
};
use std::collections::BTreeMap;

fn identity() -> PythonPopulationManifestIdentity {
    let mut env = BTreeMap::new();
    env.insert("PYTHONPATH".to_string(), "src".to_string());
    PythonPopulationManifestIdentity {
        cache_schema_version: rslip::CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: PYTHON_SELECTOR_DISCOVERY_VERSION.to_string(),
        python_version: "3.12.0".to_string(),
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env,
    }
}

#[test]
fn python_manifest_rejects_v1_selector_discovery_version() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    let mut identity = identity();
    identity.selector_discovery_version = "python-selector-discovery-v1".to_string();
    write_python_population_manifest_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    )
    .unwrap();
    assert!(
        !python_population_manifest_is_current_for_args_with_env_keys(
            tmp.path(),
            std::slice::from_ref(&selector),
            &[],
            PYTHON_COVERAGE_ENV_KEYS,
        )
    );
}

#[test]
fn python_coverage_env_tracks_only_pythonpath() {
    let _lock = crate::cwd_test_lock::lock();
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", "src");
    let _hashseed = TestEnvVarGuard::set("PYTHONHASHSEED", "123");
    let _dontwrite = TestEnvVarGuard::set("PYTHONDONTWRITEBYTECODE", "1");

    let env = relevant_python_coverage_env(PYTHON_COVERAGE_ENV_KEYS);

    assert_eq!(
        env,
        BTreeMap::from([("PYTHONPATH".to_string(), "src".to_string())])
    );
}

#[test]
fn python_population_environment_mismatch_reports_recorded_and_current_values() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", "recorded");
    let identity = current_python_population_manifest_identity_with_env_keys(
        tmp.path(),
        &[],
        PYTHON_COVERAGE_ENV_KEYS,
    )
    .unwrap();
    write_python_population_manifest_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    )
    .unwrap();
    unsafe { std::env::remove_var("PYTHONPATH") };

    assert_eq!(
        python_population_environment_mismatch(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS),
        Some((
            BTreeMap::from([("PYTHONPATH".to_string(), "recorded".to_string())]),
            BTreeMap::new(),
        ))
    );
}

#[test]
fn python_manifest_current_with_env_keys_uses_supplied_allowlist() {
    let _lock = crate::cwd_test_lock::lock();
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", "src");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    let identity =
        current_python_population_manifest_identity_with_env_keys(tmp.path(), &[], &[]).unwrap();
    write_python_population_manifest_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity,
    )
    .unwrap();

    assert!(
        python_population_manifest_is_current_for_args_with_env_keys(
            tmp.path(),
            std::slice::from_ref(&selector),
            &[],
            &[],
        )
    );
    assert!(
        !python_population_manifest_is_current_for_args_with_env_keys(
            tmp.path(),
            std::slice::from_ref(&selector),
            &[],
            PYTHON_COVERAGE_ENV_KEYS,
        )
    );
}

#[test]
fn manifest_identity_and_matching_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let mut identity = identity();
    let selector = "tests/test_app.py::test_value".to_string();

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

    let entry_path = python_coverage_cache_root(tmp.path())
        .unwrap()
        .join("entries")
        .join("new-entry.json");
    std::fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
    std::fs::write(&entry_path, "{}").unwrap();
    assert!(!python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity
    ));

    identity.python_version.clear();
    assert!(!identity.has_python_tool_versions());
    assert!(!manifest.matches_python_identity(&identity, &normalized_python_repo_root(tmp.path())));
    assert!(!python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity
    ));
}

#[test]
fn stored_python_universe_selectors_reads_current_manifest() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    assert!(stored_python_universe_selectors(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).is_none());
    write_python_population_manifest_for_args(tmp.path(), std::slice::from_ref(&selector), &[])
        .unwrap();
    let stored =
        stored_python_universe_selectors(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).unwrap();
    assert_eq!(stored, vec![selector]);

    let entry_path = python_coverage_cache_root(tmp.path())
        .unwrap()
        .join("entries")
        .join("new-entry.json");
    std::fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
    std::fs::write(&entry_path, "{}").unwrap();
    assert!(stored_python_universe_selectors(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).is_none());

    std::fs::write(tmp.path().join("new.py"), "x = 2\n").unwrap();
    assert!(stored_python_universe_selectors(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).is_none());
}

#[test]
fn stored_python_population_identity_tracks_validated_context() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    write_python_population_manifest_for_args(tmp.path(), std::slice::from_ref(&selector), &[])
        .unwrap();

    let population =
        stored_python_universe_population(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).unwrap();
    let mut manifest = read_python_population_manifest(tmp.path()).unwrap();
    let original = stable_population_identity(&manifest);
    manifest.input_fingerprint = "different".to_string();

    assert_eq!(population.selectors, vec![selector]);
    assert_eq!(population.identity, original);
    assert_ne!(stable_population_identity(&manifest), original);
}

#[test]
fn stored_python_population_rejects_duplicate_or_unsorted_selectors() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector_a = "tests/test_app.py::test_a".to_string();
    let selector_b = "tests/test_app.py::test_b".to_string();
    write_python_population_manifest_for_args(
        tmp.path(),
        &[selector_a.clone(), selector_b.clone()],
        &[],
    )
    .unwrap();
    let path = python_population_manifest_path(tmp.path()).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["selectors"] = serde_json::json!([selector_b, selector_a]);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    assert!(stored_python_universe_population(tmp.path(), &[], PYTHON_COVERAGE_ENV_KEYS).is_none());
}
