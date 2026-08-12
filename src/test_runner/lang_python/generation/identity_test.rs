//! Direct coverage for Python execution-identity helpers.

use std::fs;

use tempfile::tempdir;

use super::identity::{
    current_python_execution_identity, identity_matches_current, population_plan_for_selectors,
};
use crate::test_runner::python_coverage_index::storage::python_coverage_cache_root;
use crate::test_runner::runners::detect_rslip_versions;

#[test]
fn identity_matches_current_uses_tools_only_when_warm_seal_present() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }

    let args = vec![
        "-p".to_string(),
        "plugin_a".to_string(),
        "--tb=short".to_string(),
        "-p".to_string(),
        "plugin_b".to_string(),
    ];
    let identity = current_python_execution_identity(repo, &args).unwrap();
    assert_eq!(
        identity.plugin_identities,
        vec!["plugin_a".to_string(), "plugin_b".to_string()]
    );

    let cache_root = python_coverage_cache_root(repo).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    fs::write(cache_root.join("warm_hit_seal.json"), b"{}\n").unwrap();

    assert!(
        identity_matches_current(repo, &identity, &args),
        "warm seal should compare tools-only fields and accept a matching identity"
    );

    let mut drifted = identity.clone();
    drifted.pytest_version = "not-the-real-pytest".into();
    assert!(
        !identity_matches_current(repo, &drifted, &args),
        "tools-only path must reject tool-version drift"
    );
}

#[test]
fn population_plan_dedups_selectors_and_captures_plugin_args() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }

    let args = vec!["-p".to_string(), "only_plugin".to_string()];
    let plan = population_plan_for_selectors(
        repo,
        &["b::t".into(), "a::t".into(), "b::t".into()],
        &args,
    )
    .unwrap();
    assert_eq!(plan.selectors, vec!["a::t".to_string(), "b::t".to_string()]);
    assert_eq!(plan.base_identity.plugin_identities, vec!["only_plugin".to_string()]);
    assert!(identity_matches_current(
        repo,
        &plan.base_identity,
        &args
    ));
}
