use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::{
    RustCovCacheEntry, RustCovCacheStatus, RustCoverageBatchIdentity, RustCoverageBatchRequest,
    RustCoverageToolIdentity, RustLineCoverage, RustLlvmCovOutcome, batch_identity,
    entry_fingerprint, load_current_population_state, placeholder_delegated_runner_fields,
    population_derived_state_stale, publish_derived_state, store_rust_cov_cache_entry,
};

use super::test_support::write_test_entry;
use super::*;

fn sample_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn lib() {}\n").unwrap();
    write_test_entry(
        tmp.path(),
        "a",
        "test_lib",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    (tmp, lib)
}

#[test]
fn population_manifest_requires_matching_selectors() {
    let (tmp, _lib) = sample_repo();

    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    assert!(rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["test_lib".to_string()],
        &[],
    ));
    assert!(!rust_population_manifest_is_current_for_args(
        tmp.path(),
        &["other_test".to_string()],
        &[],
    ));
}

#[test]
fn population_manifest_invalidates_on_source_change() {
    let (tmp, lib) = sample_repo();
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();

    fs::write(&lib, "pub fn lib() {}\npub fn changed() {}\n").unwrap();
    assert!(
        !rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "source changes invalidate population freshness"
    );
}

#[test]
fn population_manifest_invalidates_on_new_entries() {
    let (tmp, lib) = sample_repo();
    write_rust_population_manifest_for_args(tmp.path(), &["test_lib".to_string()], &[]).unwrap();
    write_test_entry(
        tmp.path(),
        "b",
        "test_other",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    );
    assert!(
        !rust_population_manifest_is_current_for_args(tmp.path(), &["test_lib".to_string()], &[],),
        "new cache entries invalidate derived population state until repair"
    );
    assert!(
        !rust_population_manifest_is_current_for_args(
            tmp.path(),
            &["test_lib".to_string(), "test_other".to_string()],
            &[],
        ),
        "selector universe changes invalidate population freshness"
    );
}

fn write_minimal_demo_crate(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src").join("lib.rs"), "pub fn lib() {}\n").unwrap();
}

fn witness_tools() -> RustCoverageToolIdentity {
    RustCoverageToolIdentity {
        cargo_version: "cargo-test".to_string(),
        llvm_cov_version: "llvm-cov-test".to_string(),
        rustc_version: "rustc-test".to_string(),
        cargo_nextest_version: "nextest-test".to_string(),
    }
}

fn selective_refresh_request(root: &Path) -> RustCoverageBatchRequest {
    let (delegated_runners, runner_map_fingerprint, host_platform) =
        placeholder_delegated_runner_fields();
    RustCoverageBatchRequest {
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: root.join(".kiss").join("rust_llvm_cov_cache"),
        logical_selectors: vec!["test_lib".to_string()],
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: BTreeMap::new(),
        force_rerun: false,
        jobs: 1,
        generated_config: root.join(".kiss/rust_llvm_cov_cache/runs/plan/nextest.toml"),
        population_publication_selectors: Some(vec!["test_lib".to_string()]),
        delegated_runners,
        runner_map_fingerprint,
        host_platform,
        coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
    }
}

fn store_test_lib_coverage(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    lines: BTreeSet<u32>,
) {
    let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, "test_lib");
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "test_lib".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), lines)]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
}

fn assert_population_current(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    root: &Path,
) {
    assert!(
        !population_derived_state_stale(req, tools, identity).unwrap(),
        "derived population must be current"
    );
    assert!(
        load_current_population_state(
            &req.cache_root,
            root,
            identity,
            Some(&["test_lib".to_string()]),
        )
        .is_some(),
        "rebuilding derived state after selective entry refresh restores population freshness"
    );
}

#[test]
fn rust_forced_selective_entry_refresh_keeps_population_manifest_current_regression() {
    // Fake tools + placeholder runners: VISION ≤2s alone without Cargo runner resolve warm-up.
    let tmp = tempfile::tempdir().unwrap();
    write_minimal_demo_crate(tmp.path());
    let req = selective_refresh_request(tmp.path());
    let tools = witness_tools();
    let identity = batch_identity(&req, &tools).expect("batch identity");

    store_test_lib_coverage(&req, &tools, &identity, BTreeSet::from([1]));
    publish_derived_state(&req, &tools, &identity, &["test_lib".to_string()], false).unwrap();
    assert_population_current(&req, &tools, &identity, tmp.path());

    store_test_lib_coverage(&req, &tools, &identity, BTreeSet::from([1, 2]));
    publish_derived_state(&req, &tools, &identity, &["test_lib".to_string()], true).unwrap();
    assert_population_current(&req, &tools, &identity, tmp.path());
}
