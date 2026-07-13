use std::collections::{BTreeMap, BTreeSet};

use super::super::test_support::write_test_entry;
use super::{
    CACHE_SCHEMA_VERSION, LEGACY_POPULATION_SCHEMA_VERSION, RUST_COVERAGE_ENV_KEYS,
    RUST_SELECTOR_DISCOVERY_VERSION, RustPopulationManifest, RustPopulationManifestIdentity,
    batch_population_manifest_is_current, current_rust_population_manifest_identity,
    current_rust_population_manifest_identity_with_env_keys, relevant_rust_coverage_env,
    rust_population_manifest_is_current_for_args_with_env_keys, rust_population_manifest_path,
    write_rust_population_manifest_for_args, write_rust_population_manifest_with_identity,
};

fn test_identity() -> RustPopulationManifestIdentity {
    RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: "rustc".to_string(),
        cargo_version: "cargo".to_string(),
        cargo_llvm_cov_version: "llvm-cov".to_string(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
    }
}

#[test]
fn rust_population_manifest_identity() {
    let identity = test_identity();

    assert!(identity.has_tool_versions());
    assert_eq!(identity.tool_versions(), ["rustc", "cargo", "llvm-cov"]);
    assert!(identity.args_match(&[], &[]));
    assert!(std::mem::size_of::<RustPopulationManifestIdentity>() > 0);
}

#[test]
fn rust_population_manifest() {
    let identity = test_identity();
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
    assert!(std::mem::size_of::<RustPopulationManifest>() > 0);
}

#[test]
fn relevant_rust_coverage_env_collects_configured_keys() {
    let env = relevant_rust_coverage_env(&["RUSTFLAGS"]);
    assert!(env.len() <= 1);
}

#[test]
fn current_rust_population_manifest_identity_with_env_keys_builds_tool_versions() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    let identity = current_rust_population_manifest_identity_with_env_keys(
        tmp.path(),
        &[],
        RUST_COVERAGE_ENV_KEYS,
    )
    .unwrap();
    assert!(!identity.rustc_version.is_empty());
}

#[test]
fn write_rust_population_manifest_for_args_uses_current_identity() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["alpha".to_string()], &[]).unwrap();
    assert!(rust_population_manifest_path(tmp.path()).is_file());
    let identity = current_rust_population_manifest_identity(tmp.path(), &[]).unwrap();
    assert!(!identity.cargo_version.is_empty());
    assert!(!rust_population_manifest_is_current_for_args_with_env_keys(
        tmp.path(),
        &[],
        &["alpha".to_string()],
        RUST_COVERAGE_ENV_KEYS,
    ));
}

#[test]
fn batch_population_manifest_is_current_after_derived_state_write() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    std::fs::write(&lib, "pub fn lib() {}\n").unwrap();
    let _ = super::super::current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        rpytest_runner::TestStatus::Passed,
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    let identity = current_rust_population_manifest_identity(tmp.path(), &[]).unwrap();
    write_rust_population_manifest_with_identity(tmp.path(), &["test_lib".to_string()], &identity)
        .unwrap();
    assert!(batch_population_manifest_is_current(
        tmp.path(),
        &["test_lib".to_string()],
        &identity,
    ));
    assert!(!batch_population_manifest_is_current(
        tmp.path(),
        &["other".to_string()],
        &identity,
    ));
}

#[test]
fn batch_population_manifest_is_current_false_without_batch_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: String::new(),
        cargo_version: String::new(),
        cargo_llvm_cov_version: String::new(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
    };
    assert!(!batch_population_manifest_is_current(
        tmp.path(),
        &["test_lib".to_string()],
        &identity,
    ));
}
