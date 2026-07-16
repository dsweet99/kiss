use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::{
    BATCH_INDEX_SCHEMA_VERSION, RustCovCacheEntry, RustCovCacheStatus, RustLineCoverage,
    RustLlvmCovOutcome, generation_entries_fingerprint, repo_relative_coverage_file,
    repo_relative_path, store_rust_cov_cache_entry,
};

use crate::test_runner::rust_coverage_index::load_current_rust_coverage_index;
use crate::test_runner::rust_coverage_index::storage::{
    rust_coverage_cache_root, rust_coverage_index_path,
};
use crate::test_runner::rust_coverage_index::test_support::{
    test_entries_fingerprint, write_test_entry, write_test_entry_with_args,
};

#[test]
fn normalized_repo_root_returns_canonical_path() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical = tmp.path().canonicalize().unwrap();
    assert_eq!(
        crate::test_runner::rust_coverage_index::storage::normalized_repo_root(tmp.path()),
        canonical.to_string_lossy()
    );
}

#[test]
fn command_stdout_reports_success_and_failure() {
    let text = crate::test_runner::rust_coverage_index::storage::command_stdout(
        Path::new("printf"),
        &["ok"],
        Path::new("."),
    )
    .unwrap();

    assert_eq!(text, "ok");
    assert!(
        crate::test_runner::rust_coverage_index::storage::command_stdout(
            Path::new("/definitely/not/a/command"),
            &[],
            Path::new(".")
        )
        .is_err()
    );
    assert!(
        crate::test_runner::rust_coverage_index::storage::command_stdout(
            Path::new("false"),
            &[],
            Path::new(".")
        )
        .is_err()
    );
    assert_eq!(
        crate::test_runner::rust_coverage_index::storage::command_stdout(
            Path::new("false"),
            &[],
            Path::new(".")
        )
        .unwrap_err(),
        "error: kiss test: false failed: "
    );
}

#[test]
fn create_new_file_rejects_duplicate_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("entry.json");
    crate::test_runner::rust_coverage_index::storage::create_new_file(&path)
        .unwrap()
        .write_all(b"{}")
        .unwrap();
    assert!(crate::test_runner::rust_coverage_index::storage::create_new_file(&path).is_err());
}

#[test]
fn write_test_json_atomically_writes_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.json");
    crate::test_runner::rust_coverage_index::storage::write_test_json_atomically(
        &tmp.path().join("tmp.json"),
        &out,
        &serde_json::json!({"ok": true}),
    )
    .unwrap();
    assert!(out.is_file());
}

#[test]
fn is_cargo_config_input_path_matches_only_cargo_configs() {
    let recognizes_bare =
        rust_llvm_cov_runner::is_cargo_config_input_path(Path::new(".cargo/config"));
    let recognizes_toml =
        rust_llvm_cov_runner::is_cargo_config_input_path(Path::new(".cargo/config.toml"));
    let rejects_root = rust_llvm_cov_runner::is_cargo_config_input_path(Path::new("config.toml"));
    assert!(recognizes_bare);
    assert!(recognizes_toml);
    assert!(!rejects_root);
}

fn write_generation_scoped_derived_artifacts(
    tmp: &Path,
    identity: &rust_llvm_cov_runner::RustCoverageBatchIdentity,
    generation: &str,
    entries_fingerprint: &str,
    selectors: &[&str],
) {
    let selectors_json = selectors
        .iter()
        .map(|selector| format!("\"{selector}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let ordinary_source_digests_json = identity
        .ordinary_source_digests
        .iter()
        .map(|(path, digest)| format!(r#"{{ "path": "{path}", "digest": "{digest}" }}"#))
        .collect::<Vec<_>>()
        .join(", ");
    let index_payload = format!(
        r#"{{
  "schema_version": "{schema}",
  "source_root": "{}",
  "generation_fingerprint": "{generation}",
  "entries_fingerprint": "{entries_fingerprint}",
  "files": {{ "src/lib.rs": ["alpha"] }}
}}"#,
        tmp.canonicalize().unwrap().display(),
        schema = BATCH_INDEX_SCHEMA_VERSION,
    );
    fs::write(rust_coverage_index_path(tmp), index_payload).unwrap();
    let population_payload = format!(
        r#"{{
  "schema_version": "{population_schema}",
  "source_root": "{}",
  "input_fingerprint": "{input_fingerprint}",
  "generation_fingerprint": "{generation}",
  "selection_context_fingerprint": "{selection_context_fingerprint}",
  "entries_fingerprint": "{entries_fingerprint}",
  "selectors": [{selectors_json}],
  "ordinary_source_digests": [{ordinary_source_digests_json}],
  "test_binaries": [{{
    "id": "test-bin",
    "executable": "test-bin",
    "digest": "0000000000000000"
  }}]
}}"#,
        tmp.canonicalize().unwrap().display(),
        population_schema = rust_llvm_cov_runner::BATCH_POPULATION_SCHEMA_VERSION,
        input_fingerprint = identity.input_digest,
        selection_context_fingerprint = identity.selection_context_fingerprint,
    );
    fs::write(
        crate::test_runner::rust_coverage_index::storage::rust_population_manifest_path(tmp),
        population_payload,
    )
    .unwrap();
}

#[test]
fn generation_scoped_index_helpers_validate_entries_and_paths() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();

    assert_eq!(
        repo_relative_coverage_file(tmp.path(), &lib.to_string_lossy()),
        Some("src/lib.rs".to_string())
    );
    assert_eq!(
        repo_relative_path(tmp.path(), Path::new("src/lib.rs")),
        Some("src/lib.rs".to_string())
    );
    assert!(repo_relative_path(tmp.path(), Path::new("/outside.rs")).is_none());

    let cache_root = rust_coverage_cache_root(tmp.path());
    fs::create_dir_all(cache_root.join("entries")).unwrap();
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &[],
    )
    .expect("batch identity");
    let generation = identity.generation_fingerprint.clone();
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: std::time::Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &generation,
    );
    store_rust_cov_cache_entry(&cache_root, "abc123", &entry).unwrap();
    let entries_fingerprint = generation_entries_fingerprint(&cache_root, &generation).unwrap();
    assert!(!entries_fingerprint.is_empty());
    write_generation_scoped_derived_artifacts(
        tmp.path(),
        &identity,
        &generation,
        &entries_fingerprint,
        &["alpha"],
    );
    let loaded = load_current_rust_coverage_index(tmp.path(), &[]).expect("generation index");
    assert!(loaded.contains_key("src/lib.rs"));
}

#[test]
fn current_index_loader_uses_requested_rust_test_args() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let exact_args = vec!["--exact".to_string()];
    write_test_entry_with_args(
        tmp.path(),
        "exact",
        "alpha",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
        &exact_args,
    );
    let identity = crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
        tmp.path(),
        &exact_args,
    )
    .expect("batch identity");
    let entries_fingerprint = test_entries_fingerprint(tmp.path(), &exact_args);
    write_generation_scoped_derived_artifacts(
        tmp.path(),
        &identity,
        &identity.generation_fingerprint,
        &entries_fingerprint,
        &["alpha"],
    );

    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());
    assert!(load_current_rust_coverage_index(tmp.path(), &exact_args).is_some());
}

#[test]
fn write_rust_coverage_index_writes_generation_scoped_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "entry",
        "alpha",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                tmp.path().join("src/lib.rs").to_string_lossy().to_string(),
                BTreeSet::from([1]),
            )]),
        },
    );
    let index = BTreeMap::from([(
        "src/lib.rs".to_string(),
        BTreeSet::from(["alpha".to_string()]),
    )]);
    super::write_rust_coverage_index(tmp.path(), &index).unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_some());
}

#[test]
fn workspace_input_fingerprint_matches_runner_shared_input_digest() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    assert_eq!(
        super::workspace_input_fingerprint(tmp.path()).unwrap(),
        rust_llvm_cov_runner::workspace_input_digest(tmp.path()).unwrap()
    );
}

#[test]
fn write_rust_coverage_index_writes_index_and_population() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    let index = BTreeMap::from([(
        "src/lib.rs".to_string(),
        BTreeSet::from(["tests::one".to_string()]),
    )]);
    crate::test_runner::rust_coverage_index::write_rust_coverage_index(tmp.path(), &index).unwrap();
    assert!(rust_coverage_index_path(tmp.path()).is_file());
    assert!(
        crate::test_runner::rust_coverage_index::rust_population_manifest_path(tmp.path())
            .is_file()
    );
}

fn empty_coverage() -> RustLineCoverage {
    RustLineCoverage {
        files: BTreeMap::new(),
    }
}

#[test]
fn entries_fingerprint_tracks_multiple_entries() {
    let tmp = tempfile::tempdir().unwrap();
    write_test_entry(tmp.path(), "a", "one", TestStatus::Passed, empty_coverage());
    write_test_entry(tmp.path(), "b", "two", TestStatus::Passed, empty_coverage());
    let first = test_entries_fingerprint(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "c",
        "three",
        TestStatus::Passed,
        empty_coverage(),
    );
    let second = test_entries_fingerprint(tmp.path(), &[]);
    assert_ne!(first, second);
}
