use super::*;
use crate::test_runner::python_coverage_index::storage::normalized_python_repo_root;
use std::collections::BTreeMap;

struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

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
fn python_coverage_env_tracks_only_pythonpath() {
    let _lock = crate::cwd_test_lock::lock();
    let _pythonpath = EnvGuard::set("PYTHONPATH", "src");
    let _hashseed = EnvGuard::set("PYTHONHASHSEED", "123");
    let _dontwrite = EnvGuard::set("PYTHONDONTWRITEBYTECODE", "1");

    let env = relevant_python_coverage_env();

    assert_eq!(
        env,
        BTreeMap::from([("PYTHONPATH".to_string(), "src".to_string())])
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

    identity.python_version.clear();
    assert!(!identity.has_python_tool_versions());
    assert!(!manifest.matches_python_identity(&identity, &normalized_python_repo_root(tmp.path())));
    assert!(!python_population_manifest_is_current_with_identity(
        tmp.path(),
        std::slice::from_ref(&selector),
        &identity
    ));
}
