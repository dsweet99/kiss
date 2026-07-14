use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::*;

#[test]
fn witness_build_test_rust_coverage_index() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    super::test_support::write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        rpytest_runner::TestStatus::Passed,
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    let index = build_test_rust_coverage_index(tmp.path()).unwrap();
    assert!(
        index
            .get("src/lib.rs")
            .is_some_and(|s| s.contains("test_lib"))
    );
}

#[test]
fn witness_changed_line_rels() {
    let tmp = tempfile::tempdir().unwrap();
    let mut changed = BTreeMap::new();
    changed.insert(tmp.path().join("src/lib.rs"), BTreeSet::from([1]));
    let rels = changed_line_rels(tmp.path(), &changed);
    assert!(rels.contains_key("src/lib.rs"));
}

#[test]
fn witness_changed_line_selection_ignores_retained_prior_generation_entries() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn a() {}\npub fn b() {}\n#[cfg(test)] mod tests { #[test] fn test_current_line() {} #[test] fn test_stale_line() {} }\n",
    )
    .unwrap();
    let _ = super::current_rust_coverage_batch_identity(tmp.path(), &[]);
    super::test_support::write_test_entry(
        tmp.path(),
        "current",
        "tests::test_current_line",
        rpytest_runner::TestStatus::Passed,
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([2]))]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(
        tmp.path(),
        &["tests::test_current_line".to_string()],
        &[],
    )
    .unwrap();
    let stale_path = rust_coverage_cache_root(tmp.path())
        .join("entries")
        .join("stale.json");
    fs::write(
        stale_path,
        serde_json::json!({
            "schema_version": CACHE_SCHEMA_VERSION,
            "generation_fingerprint": "prior-complete-generation",
            "selector": "tests::test_stale_line",
            "status": rpytest_runner::TestStatus::Passed,
            "exit_code": 0,
            "duration": 1,
            "coverage": {
                "files": {
                    "src/lib.rs": [1]
                }
            },
        })
        .to_string(),
    )
    .unwrap();

    let selected = select_rust_source_selectors_for_changed_lines(
        tmp.path(),
        &BTreeMap::from([(lib.clone(), BTreeSet::from([1]))]),
    );

    assert!(
        selected.is_none(),
        "retained prior-generation line coverage must not select tests"
    );
    assert_eq!(
        select_rust_source_selectors_hybrid(
            tmp.path(),
            std::slice::from_ref(&lib),
            &BTreeMap::from([(lib.clone(), BTreeSet::from([1]))]),
            &[],
        ),
        Some(BTreeSet::from(["tests::test_current_line".to_string()]))
    );
}

#[test]
fn witness_manifest_identity_and_private_helpers() {
    let identity: RustPopulationManifestIdentity = RustPopulationManifestIdentity {
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        selector_discovery_version: RUST_SELECTOR_DISCOVERY_VERSION.to_string(),
        rustc_version: "rustc".to_string(),
        cargo_version: "cargo".to_string(),
        cargo_llvm_cov_version: "llvm-cov".to_string(),
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
    };
    let manifest: RustPopulationManifest = RustPopulationManifest {
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
        selectors: Vec::new(),
    };
    assert!(identity.has_tool_versions());
    assert_eq!(identity.tool_versions(), ["rustc", "cargo", "llvm-cov"]);
    assert!(identity.args_match(&[], &[]));
    assert!(manifest.matches_identity(&identity, "root"));
    assert!(manifest.matches_selectors(&[]));
    assert_eq!(manifest.cache_schema_version, CACHE_SCHEMA_VERSION);
    assert!(command_stdout(Path::new("/definitely/not/a/command"), &[], Path::new(".")).is_err());
    assert_eq!(command_output_text(b" ok \n"), "ok");
    assert!(command_failure_message(Path::new("cmd"), b"bad").contains("cmd failed: bad"));
    assert!(is_cargo_config_input_path(Path::new(".cargo/config.toml")));
    assert!(is_cargo_config_input_path(Path::new(".cargo/config")));
    assert!(!is_cargo_config_input_path(Path::new("config.toml")));
}
