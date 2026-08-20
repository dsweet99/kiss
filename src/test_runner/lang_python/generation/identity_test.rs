use std::fs;

use tempfile::tempdir;

use super::identity::{
    current_python_execution_identity, identity_matches_current, population_plan_for_selectors,
};
use crate::test_runner::runners::detect_rslip_versions;

#[test]
fn identity_matches_current_requires_full_equality_including_fingerprint() {
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
    super::identity_memo::clear_python_execution_identity_memo();
    let identity = current_python_execution_identity(repo, &args).unwrap();
    assert_eq!(
        identity.plugin_identities,
        vec!["plugin_a".to_string(), "plugin_b".to_string()]
    );

    assert!(
        identity_matches_current(repo, &identity, &args),
        "matching identity must be accepted"
    );

    let mut drifted_tools = identity.clone();
    drifted_tools.pytest_version = "not-the-real-pytest".into();
    assert!(
        !identity_matches_current(repo, &drifted_tools, &args),
        "tool-version drift must be rejected"
    );

    let mut drifted_fp = identity.clone();
    drifted_fp.input_fingerprint = "stale-fingerprint".into();
    assert!(
        !identity_matches_current(repo, &drifted_fp, &args),
        "fingerprint drift must be rejected even when tools match"
    );
}

#[test]
fn execution_identity_is_memoized_within_cycle() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    if detect_rslip_versions(repo).is_err() {
        return;
    }
    let args: Vec<String> = vec![];
    super::identity_memo::clear_python_execution_identity_memo();
    let first = current_python_execution_identity(repo, &args).unwrap();

    fs::write(repo.join("app.py"), b"x = 2\n").unwrap();
    let second = current_python_execution_identity(repo, &args).unwrap();
    assert_eq!(
        first.input_fingerprint, second.input_fingerprint,
        "identity must be reused within a cycle"
    );
    super::identity_memo::clear_python_execution_identity_memo();
    let third = current_python_execution_identity(repo, &args).unwrap();
    assert_ne!(
        first.input_fingerprint, third.input_fingerprint,
        "clearing memo must recompute after source change"
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
    let plan =
        population_plan_for_selectors(repo, &["b::t".into(), "a::t".into(), "b::t".into()], &args)
            .unwrap();
    assert_eq!(plan.selectors, vec!["a::t".to_string(), "b::t".to_string()]);
    assert_eq!(
        plan.base_identity.plugin_identities,
        vec!["only_plugin".to_string()]
    );
    assert!(identity_matches_current(repo, &plan.base_identity, &args));
}
