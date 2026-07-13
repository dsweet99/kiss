use super::*;
use crate::batch_fingerprint::entry_fingerprint;
use crate::rust_cov_cache::store_rust_cov_cache_entry;
use crate::test_support::{derived_fixture_request, store_alpha_entry, witness_batch_tools};
use crate::{RustCovCacheStatus, RustLlvmCovOutcome};
use std::path::Path;
use std::time::Duration;

#[test]
fn derived_repair_rebuilds_index_from_current_generation_only() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    let mut coverage = BTreeMap::new();
    coverage.insert("src/lib.rs".to_string(), BTreeSet::from([2]));
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files: coverage },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();

    let counters =
        publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true).unwrap();
    assert!(counters.derived_repair);
    assert_eq!(
        counters.current_index_generation,
        identity.generation_fingerprint
    );

    let index_bytes = std::fs::read(req.cache_root.join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_slice(&index_bytes).unwrap();
    assert_eq!(
        index["generation_fingerprint"].as_str(),
        Some(identity.generation_fingerprint.as_str())
    );
    assert!(String::from_utf8_lossy(&index_bytes).contains("src/lib.rs"));
}

#[test]
fn population_derived_state_stale_when_manifest_missing() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    assert!(population_derived_state_stale(&req, &tools, &identity).unwrap());
}

#[test]
fn try_publish_skips_when_derived_state_current() {
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
    publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], false).unwrap();
    assert!(
        try_publish_population_derived_state(&req, &tools, &identity, &["alpha".to_string()])
            .unwrap()
            .is_none()
    );
}

#[test]
fn publish_derived_state_retains_one_previous_complete_generation() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let previous = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "legacy".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        "previous-generation",
    );
    store_rust_cov_cache_entry(&req.cache_root, "cafebabecafebabe", &previous).unwrap();
    std::fs::create_dir_all(req.cache_root.join("entries")).unwrap();
    write_population_manifest_for_test(
        &req.cache_root,
        "previous-generation",
        &["legacy".to_string()],
    );
    let counters =
        publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true).unwrap();
    assert_eq!(counters.cache_pruned_entries, 0);
    assert!(
        req.cache_root
            .join("entries")
            .join("cafebabecafebabe.json")
            .is_file()
    );
}

fn write_population_manifest_for_test(cache_root: &Path, generation: &str, selectors: &[String]) {
    let payload = serde_json::json!({
        "schema_version": POPULATION_SCHEMA_VERSION,
        "generation_fingerprint": generation,
        "selectors": selectors,
    });
    std::fs::write(
        cache_root.join("population.json"),
        serde_json::to_vec_pretty(&payload).unwrap(),
    )
    .unwrap();
}

#[test]
fn publish_derived_state_prunes_stale_generation_entries() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let stale = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "stale".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        "old-generation",
    );
    store_rust_cov_cache_entry(&req.cache_root, "deadbeefdeadbeef", &stale).unwrap();
    let counters =
        publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true).unwrap();
    assert_eq!(counters.cache_pruned_entries, 1);
}

fn write_generation_transition_repo(repo: &Path) {
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.join("src").join("a.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(repo.join("src").join("b.rs"), "pub fn b() {}\n").unwrap();
}

#[test]
fn generation_transition_index_excludes_prior_generation_files() {
    let repo = tempfile::tempdir().unwrap();
    write_generation_transition_repo(repo.path());
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let gen_a = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    store_alpha_entry(
        &req.cache_root,
        &req,
        &tools,
        &gen_a,
        BTreeMap::from([("src/a.rs".to_string(), BTreeSet::from([1]))]),
    );
    publish_derived_state(&req, &tools, &gen_a, &["alpha".to_string()], false).unwrap();
    assert!(index_contains_file(&req.cache_root, "src/a.rs"));

    let req_b = req.clone();
    std::fs::write(repo.path().join("src").join("b.rs"), "pub fn b() {}\npub fn c() {}\n")
        .unwrap();
    let gen_b = crate::batch_fingerprint::batch_identity(&req_b, &tools).unwrap();
    assert_ne!(gen_a.generation_fingerprint, gen_b.generation_fingerprint);
    store_alpha_entry(
        &req.cache_root,
        &req_b,
        &tools,
        &gen_b,
        BTreeMap::from([("src/b.rs".to_string(), BTreeSet::from([1]))]),
    );
    publish_derived_state(&req_b, &tools, &gen_b, &["alpha".to_string()], true).unwrap();
    assert!(!index_contains_file(&req.cache_root, "src/a.rs"));
    assert!(index_contains_file(&req.cache_root, "src/b.rs"));
    assert_eq!(
        read_index_json(&req.cache_root)["generation_fingerprint"].as_str(),
        Some(gen_b.generation_fingerprint.as_str())
    );
}

fn index_contains_file(cache_root: &Path, file: &str) -> bool {
    read_index_json(cache_root)["files"]
        .as_object()
        .is_some_and(|files| files.contains_key(file))
}

fn read_index_json(cache_root: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(cache_root.join("index.json")).unwrap()).unwrap()
}

#[test]
fn try_publish_all_hit_repair_when_manifest_stale() {
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
    let publish =
        try_publish_population_derived_state(&req, &tools, &identity, &["alpha".to_string()])
            .unwrap()
            .expect("stale derived state should publish");
    assert!(publish.derived_repair);
    assert_eq!(publish.entry_generation_count, 1);
}

#[test]
fn prune_obsolete_selective_generations_retains_population_and_current() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity =
        crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    publish_derived_state(
        &req,
        &tools,
        &population_identity,
        &["alpha".to_string()],
        false,
    )
    .unwrap();
    let population_generation = population_identity.generation_fingerprint.clone();
    let stale = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "stale".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        "obsolete-generation",
    );
    store_rust_cov_cache_entry(&req.cache_root, "deadbeefdeadbeef", &stale).unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let selective_identity =
        crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let pruned = prune_obsolete_selective_generations(
        &req.cache_root,
        &selective_identity.generation_fingerprint,
    )
    .unwrap();
    assert_eq!(pruned, 1);
    assert!(
        req.cache_root
            .join("entries")
            .join("deadbeefdeadbeef.json")
            .exists()
            == false
    );
    let manifest = crate::batch_derived_index::read_population_manifest(&req.cache_root)
        .expect("population manifest");
    assert_eq!(
        manifest.generation_fingerprint,
        population_generation
    );
}
