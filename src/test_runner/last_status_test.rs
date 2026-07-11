use super::*;
use crate::test_runner::last_status::{
    has_language_records, prior_failures, python_last_status_identity, record_statuses,
    rust_last_status_identity,
};
use std::fs;
use std::io::Write;

fn identity() -> LastStatusIdentity {
    python_last_status_identity("3.12.0", "8.0.0", &[])
}

#[test]
fn failed_status_is_reported_as_prior_failure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let statuses = vec![(
        "tests/test_app.py::test_failed".to_string(),
        rpytest_runner::TestStatus::Failed,
    )];

    record_statuses(tmp.path(), Language::Python, &identity(), &statuses).unwrap();

    assert_eq!(
        prior_failures(tmp.path(), Language::Python, &identity()).unwrap(),
        vec!["tests/test_app.py::test_failed".to_string()]
    );
    assert!(has_language_records(tmp.path(), Language::Python).unwrap());
}

#[test]
fn passing_status_clears_prior_failure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let selector = "tests/test_app.py::test_failed".to_string();
    record_statuses(
        tmp.path(),
        Language::Python,
        &identity(),
        &[(selector.clone(), rpytest_runner::TestStatus::Failed)],
    )
    .unwrap();

    record_statuses(
        tmp.path(),
        Language::Python,
        &identity(),
        &[(selector, rpytest_runner::TestStatus::Passed)],
    )
    .unwrap();

    assert!(
        prior_failures(tmp.path(), Language::Python, &identity())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn identity_mismatch_does_not_select_prior_failure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let other_identity = python_last_status_identity("3.13.0", "8.0.0", &[]);
    record_statuses(
        tmp.path(),
        Language::Python,
        &identity(),
        &[(
            "tests/test_app.py::test_failed".to_string(),
            rpytest_runner::TestStatus::Failed,
        )],
    )
    .unwrap();

    assert!(
        prior_failures(tmp.path(), Language::Python, &other_identity)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rust_records_are_language_scoped_and_deduped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rust_identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.99",
        &[],
        "0000000000000000",
    );
    let selector = "crate::tests::failed".to_string();

    record_statuses(
        tmp.path(),
        Language::Rust,
        &rust_identity,
        &[
            (selector.clone(), rpytest_runner::TestStatus::Failed),
            (selector.clone(), rpytest_runner::TestStatus::Failed),
        ],
    )
    .unwrap();

    assert_eq!(
        prior_failures(tmp.path(), Language::Rust, &rust_identity).unwrap(),
        vec![selector]
    );
    assert!(
        prior_failures(tmp.path(), Language::Python, &identity())
            .unwrap()
            .is_empty()
    );
    assert!(has_language_records(tmp.path(), Language::Rust).unwrap());
    assert!(!has_language_records(tmp.path(), Language::Python).unwrap());
}

#[test]
fn rust_identity_includes_nextest_version() {
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.99",
        &["--exact".to_string()],
        "0000000000000000",
    );
    let serialized = serde_json::to_string(&identity).unwrap();

    assert!(serialized.contains("cargo 1.88.0"));
    assert!(serialized.contains("cargo-llvm-cov 0.6.0"));
    assert!(serialized.contains("rustc 1.88.0"));
    assert!(serialized.contains("cargo-nextest 0.9.99"));
    assert!(serialized.contains("cache-schema"));
    assert!(serialized.contains("execution-policy"));
    assert!(serialized.contains("runner-map"));
    assert!(serialized.contains("--exact"));
}

#[test]
fn helper_contracts_are_explicit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let suffix = unique_suffix();
    assert!(!suffix.is_empty());

    let path = tmp.path().join("created-once");
    let mut file = create_new_file(&path).unwrap();
    file.write_all(b"ok").unwrap();
    drop(file);

    assert!(create_new_file(&path).is_err());
}

#[test]
fn prior_lookup_and_language_names_have_direct_witnesses() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lookup: fn(&std::path::Path, Language, &LastStatusIdentity) -> Result<Vec<String>, String> =
        prior_failures;

    assert!(
        lookup(tmp.path(), Language::Python, &identity())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn invalid_schema_fails_fast() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = last_status_path(tmp.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        serde_json::json!({
            "schema_version": "other",
            "records": []
        })
        .to_string(),
    )
    .unwrap();

    let err = prior_failures(tmp.path(), Language::Python, &identity()).unwrap_err();

    assert!(err.contains("unsupported last-status schema"));
}

#[test]
fn on_disk_store_shapes_are_explicit() {
    let identity = identity();
    let record = LastStatusRecord {
        language: "python".to_string(),
        selector: "tests/test_app.py::test_failed".to_string(),
        identity: identity.clone(),
    };
    let store = LastStatusStore {
        schema_version: LAST_STATUS_SCHEMA_VERSION.to_string(),
        records: vec![record],
    };

    assert_eq!(store.records.len(), 1);
    assert_eq!(store.records[0].identity, identity);
}
