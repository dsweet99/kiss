use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::manifest::{RustPopulationManifest, RustPopulationManifestIdentity};
use super::test_support::{test_entries_fingerprint, write_test_entry, write_test_entry_with_args};
use super::*;

fn fake_manifest_identity() -> RustPopulationManifestIdentity {
    RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: "rustc 1.88.0\nhost: x86_64-unknown-linux-gnu".to_string(),
        cargo_version: "cargo 1.88.0".to_string(),
        cargo_llvm_cov_version: "cargo-llvm-cov 0.6.16".to_string(),
        cargo_args: Vec::new(),
        test_args: vec!["--exact".to_string()],
        env: BTreeMap::from([("RUSTFLAGS".to_string(), "-Cinstrument-coverage".to_string())]),
    }
}

#[test]
fn rust_population_manifest_identity() {
    let identity = fake_manifest_identity();

    assert!(identity.has_tool_versions());
    assert_eq!(
        identity.tool_versions(),
        [
            "rustc 1.88.0\nhost: x86_64-unknown-linux-gnu",
            "cargo 1.88.0",
            "cargo-llvm-cov 0.6.16"
        ]
    );
    assert!(identity.args_match(&[], &["--exact".to_string()]));
}

#[test]
fn rust_population_manifest() {
    let identity = fake_manifest_identity();
    let manifest = RustPopulationManifest {
        schema_version: LEGACY_POPULATION_SCHEMA_VERSION.to_string(),
        cache_schema_version: identity.cache_schema_version.clone(),
        source_root: "root".to_string(),
        selector_discovery_version: identity.selector_discovery_version.clone(),
        rustc_version: identity.rustc_version.clone(),
        cargo_version: identity.cargo_version.clone(),
        cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
        cargo_args: identity.cargo_args.clone(),
        test_args: identity.test_args.clone(),
        env: identity.env.clone(),
        input_fingerprint: "input".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["test_lib".to_string()],
    };

    assert!(manifest.matches_identity(&identity, "root"));
    assert!(manifest.matches_selectors(&["test_lib".to_string()]));
}

#[test]
fn write_rust_population_manifest_for_args_marks_population_current() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let _ = super::current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["missing_selector".to_string()],
        &[],
    ));
}

#[test]
fn batch_population_manifest_is_current_rejects_missing_batch_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = fake_manifest_identity();
    assert!(!rust_population_manifest_is_current_with_identity(
        tmp.path(),
        &["test_lib".to_string()],
        &identity,
    ));
}

#[test]
fn population_manifest_requires_matching_generation_and_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let identity = fake_manifest_identity();
    write_test_entry_with_args(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
        &identity.test_args,
    );

    write_rust_population_manifest_with_identity(tmp.path(), &["test_lib".to_string()], &identity)
        .unwrap();

    assert!(rust_population_manifest_is_current_with_identity(
        tmp.path(),
        &["test_lib".to_string()],
        &identity
    ));

    let mut changed = identity.clone();
    changed.test_args.push("--nocapture".to_string());
    assert!(
        !rust_population_manifest_is_current_with_identity(
            tmp.path(),
            &["test_lib".to_string()],
            &changed,
        ),
        "coverage-affecting test args invalidate population freshness"
    );

    assert!(
        !rust_population_manifest_is_current_with_identity(
            tmp.path(),
            &["other_selector".to_string()],
            &identity,
        ),
        "selector population mismatch invalidates population freshness"
    );
}

#[test]
fn empty_sources_and_empty_cache_have_empty_selection_contracts() {
    let tmp = tempfile::tempdir().unwrap();

    let index = rebuild_rust_coverage_index(tmp.path()).unwrap();
    let selected = select_rust_source_selectors_from_index(tmp.path(), &[], &[]).unwrap();

    assert!(index.is_empty());
    assert!(selected.is_empty());
    assert!(rust_coverage_cache_root(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache"));
    assert!(rust_coverage_index_path(tmp.path()).ends_with(".kiss/rust_llvm_cov_cache/index.json"));
}

#[test]
fn bad_index_files_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let index_path = rust_coverage_index_path(tmp.path());
    fs::create_dir_all(index_path.parent().unwrap()).unwrap();
    fs::write(&index_path, "{not json").unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": "old",
            "source_root": normalized_repo_root(tmp.path()),
            "entries_fingerprint": test_entries_fingerprint(tmp.path(), &[]),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());

    fs::write(
        &index_path,
        serde_json::json!({
            "schema_version": LEGACY_INDEX_SCHEMA_VERSION,
            "source_root": "/not/this/repo",
            "entries_fingerprint": test_entries_fingerprint(tmp.path(), &[]),
            "files": {}
        })
        .to_string(),
    )
    .unwrap();
    assert!(load_current_rust_coverage_index(tmp.path(), &[]).is_none());
}

#[test]
fn path_fingerprint_and_temp_file_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("other.rs");
    fs::write(&outside_file, "pub fn other() {}\n").unwrap();

    assert_eq!(
        repo_relative_coverage_file(tmp.path(), &lib.to_string_lossy()),
        Some("src/lib.rs".to_string())
    );
    assert_eq!(
        repo_relative_path(tmp.path(), Path::new("src/lib.rs")),
        Some("src/lib.rs".to_string())
    );
    assert!(repo_relative_path(tmp.path(), &outside_file).is_none());

    let first = test_entries_fingerprint(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    let second = test_entries_fingerprint(tmp.path(), &[]);
    assert_ne!(first, second);

    let temp_path = tmp.path().join("created-once.tmp");
    let _file = create_new_file(&temp_path).unwrap();
    assert!(create_new_file(&temp_path).is_err());
    assert_ne!(unique_suffix(), "");
}

#[test]
fn manifest_identity_and_private_helpers_have_contracts() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = fake_manifest_identity();

    assert_eq!(identity.cache_schema_version, CACHE_SCHEMA_VERSION);
    let manifest = RustPopulationManifest {
        schema_version: LEGACY_POPULATION_SCHEMA_VERSION.to_string(),
        cache_schema_version: identity.cache_schema_version.clone(),
        source_root: "root".to_string(),
        selector_discovery_version: identity.selector_discovery_version.clone(),
        rustc_version: identity.rustc_version.clone(),
        cargo_version: identity.cargo_version.clone(),
        cargo_llvm_cov_version: identity.cargo_llvm_cov_version.clone(),
        cargo_args: identity.cargo_args.clone(),
        test_args: identity.test_args.clone(),
        env: identity.env.clone(),
        input_fingerprint: "input".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["test_lib".to_string()],
    };
    assert_eq!(manifest.cache_schema_version, CACHE_SCHEMA_VERSION);
    assert_eq!(
        std::mem::size_of_val(&identity),
        std::mem::size_of::<RustPopulationManifestIdentity>()
    );

    let err = command_stdout(Path::new("/definitely/not/a/command"), &[], tmp.path()).unwrap_err();
    assert!(err.contains("failed to spawn"));

    assert!(is_cargo_config_input_path(Path::new(".cargo/config")));
    assert!(is_cargo_config_input_path(Path::new(".cargo/config.toml")));
    assert!(!is_cargo_config_input_path(Path::new("config.toml")));
}
