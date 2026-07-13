use crate::batch_fingerprint::entry_fingerprint;
use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
use crate::test_support::{derived_fixture_request, witness_batch_tools};
use crate::{RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome};

use super::*;

fn tamper_json_file(cache_root: &Path, relative: &str, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = cache_root.join(relative);
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn republish_alpha(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) {
    publish_derived_state(req, tools, identity, &["alpha".to_string()], true).unwrap();
}

#[test]
fn derived_state_stale_detects_manifest_mismatches() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], false).unwrap();

    tamper_json_file(&req.cache_root, "population.json", |value| {
        value["schema_version"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
    republish_alpha(&req, &tools, &identity);

    tamper_json_file(&req.cache_root, "population.json", |value| {
        value["input_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
    republish_alpha(&req, &tools, &identity);

    tamper_json_file(&req.cache_root, "population.json", |value| {
        value["selectors"] = serde_json::json!(["other"]);
    });
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
}

#[test]
fn derived_state_stale_detects_index_mismatch_and_repairs() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], false).unwrap();

    tamper_json_file(&req.cache_root, "index.json", |value| {
        value["generation_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
    republish_alpha(&req, &tools, &identity);

    tamper_json_file(&req.cache_root, "index.json", |value| {
        value["entries_fingerprint"] = serde_json::Value::String("wrong".to_string());
    });

    let (generation, selectors) =
        crate::batch_derived_index::read_population_manifest(&req.cache_root)
            .map(|manifest| (manifest.generation_fingerprint, manifest.selectors))
            .expect("manifest fields");
    assert_eq!(generation, identity.generation_fingerprint);
    assert_eq!(selectors, ["alpha".to_string()]);

    let counters =
        try_publish_population_derived_state(&req, &tools, &identity, &["alpha".to_string()])
            .unwrap()
            .expect("repair after index mismatch");
    assert!(counters.derived_repair);

    std::fs::remove_file(req.cache_root.join("index.json")).unwrap();
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
}

#[test]
fn derived_publish_counters_and_manifest_loader_are_exercised() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
            },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
    let counters =
        publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], false).unwrap();
    assert_eq!(counters.entry_generation_count, 1);
    assert_eq!(counters.cache_pruned_entries, 0);
    assert!(!counters.derived_repair);
    assert_eq!(
        counters.current_index_generation,
        identity.generation_fingerprint
    );
    let manifest =
        crate::batch_derived_index::read_population_manifest(&req.cache_root).expect("manifest");
    assert_eq!(manifest.schema_version, POPULATION_SCHEMA_VERSION);
    assert_eq!(
        manifest.generation_fingerprint,
        identity.generation_fingerprint
    );
    assert_eq!(manifest.input_fingerprint, identity.input_digest);
    assert_eq!(manifest.selectors, ["alpha".to_string()]);
    assert!(!manifest.entries_fingerprint.is_empty());
    let index = crate::batch_derived_index::read_coverage_index(&req.cache_root).expect("index");
    assert_eq!(index.schema_version, INDEX_SCHEMA_VERSION);
    assert_eq!(index.entries_fingerprint, manifest.entries_fingerprint);
}

#[test]
fn read_population_and_index_loaders_reject_invalid_json() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(crate::batch_derived_index::read_population_manifest(tmp.path()).is_none());
    std::fs::write(tmp.path().join("population.json"), b"{").unwrap();
    std::fs::write(tmp.path().join("index.json"), b"[").unwrap();
    assert!(crate::batch_derived_index::read_population_manifest(tmp.path()).is_none());
    assert!(crate::batch_derived_index::read_coverage_index(tmp.path()).is_none());
    let counters = DerivedPublishCounters::default();
    assert_eq!(counters.cache_pruned_entries, 0);

    let index = crate::batch_derived_index::OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION.to_string(),
        generation_fingerprint: "gen".to_string(),
        entries_fingerprint: "entries".to_string(),
    };
    let manifest = crate::batch_derived_index::PopulationManifestOnDisk {
        schema_version: POPULATION_SCHEMA_VERSION.to_string(),
        generation_fingerprint: "gen".to_string(),
        input_fingerprint: "input".to_string(),
        selection_context_fingerprint: "context".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["alpha".to_string()],
    };
    assert_eq!(
        index.generation_fingerprint,
        manifest.generation_fingerprint
    );
}

fn alpha_entry(
    generation: &str,
    files: BTreeMap<String, BTreeSet<u32>>,
) -> RustCovCacheEntry {
    RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        generation,
    )
}

fn write_minimal_derived_repo(repo: &Path) {
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
}

fn read_index_json(cache_root: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(cache_root.join("index.json")).unwrap()).unwrap()
}

#[test]
fn concurrent_repairers_observe_single_repair() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_derived_repo(repo.path());
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    store_rust_cov_cache_entry(
        &req.cache_root,
        &fingerprint,
        &alpha_entry(
            &identity.generation_fingerprint,
            BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        ),
    )
    .unwrap();
    std::fs::remove_file(req.cache_root.join("population.json")).ok();
    std::fs::remove_file(req.cache_root.join("index.json")).ok();

    let selectors = ["alpha".to_string()];
    let first = std::thread::spawn({
        let req = req.clone();
        let tools = tools.clone();
        let identity = identity.clone();
        let selectors = selectors.clone();
        move || try_publish_population_derived_state(&req, &tools, &identity, &selectors)
    });
    let second = std::thread::spawn({
        let req = req.clone();
        let tools = tools.clone();
        let identity = identity.clone();
        let selectors = selectors.clone();
        move || try_publish_population_derived_state(&req, &tools, &identity, &selectors)
    });
    let repaired = [first.join().expect("first"), second.join().expect("second")]
        .into_iter()
        .filter_map(Result::ok)
        .flatten()
        .collect::<Vec<_>>();
    assert!(!repaired.is_empty());
    assert!(repaired.iter().any(|counters| counters.derived_repair));
    assert!(population_manifest_state_is_current(
        &req.cache_root,
        &req.source_root,
        &identity,
        &selectors
    )
    .unwrap());
}

#[test]
fn publish_derived_state_fails_when_retention_pruning_cannot_remove_stale_entry() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_derived_repo(repo.path());
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    store_rust_cov_cache_entry(
        &req.cache_root,
        &fingerprint,
        &alpha_entry(
            &identity.generation_fingerprint,
            BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        ),
    )
    .unwrap();
    let stale_fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "stale");
    store_rust_cov_cache_entry(
        &req.cache_root,
        &stale_fingerprint,
        &alpha_entry(
            "stale-generation",
            BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([2]))]),
        ),
    )
    .unwrap();
    let entries_dir = req.cache_root.join("entries");
    let mut permissions = std::fs::metadata(&entries_dir).unwrap().permissions();
    permissions.set_mode(0o555);
    std::fs::set_permissions(&entries_dir, permissions).unwrap();
    let err = publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], false)
        .unwrap_err();
    let mut permissions = std::fs::metadata(&entries_dir).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&entries_dir, permissions).unwrap();
    assert!(matches!(err, RustLlvmCovError::Io(_)));
}

#[test]
fn publish_derived_state_fails_when_population_manifest_parent_is_not_writable() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_derived_repo(repo.path());
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    store_rust_cov_cache_entry(
        &req.cache_root,
        &fingerprint,
        &alpha_entry(
            &identity.generation_fingerprint,
            BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        ),
    )
    .unwrap();
    std::fs::create_dir(req.cache_root.join("population.json")).unwrap();
    let err = publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true)
        .unwrap_err();
    assert!(matches!(err, RustLlvmCovError::Io(_)));
}

#[test]
fn publish_derived_state_fails_when_index_parent_is_not_writable() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_derived_repo(repo.path());
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    store_rust_cov_cache_entry(
        &req.cache_root,
        &fingerprint,
        &alpha_entry(
            &identity.generation_fingerprint,
            BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        ),
    )
    .unwrap();
    std::fs::remove_file(req.cache_root.join("index.json")).ok();
    std::fs::create_dir(req.cache_root.join("index.json")).unwrap();
    let err = publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true)
        .unwrap_err();
    assert!(matches!(err, RustLlvmCovError::Io(_)));
}

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;
