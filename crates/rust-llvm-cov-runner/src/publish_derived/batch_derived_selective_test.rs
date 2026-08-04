use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;

use crate::plan::batch_fingerprint::entry_fingerprint;
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
use crate::test_support::{derived_fixture_request, store_alpha_entry, witness_batch_tools};
use crate::{
    RustCovCacheStatus, RustCoverageBatchIdentity, RustCoverageToolIdentity, RustLineCoverage,
    RustLlvmCovOutcome, publish_derived_state,
};

#[test]
fn selective_generation_store_leaves_population_manifest_unchanged() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_crate(repo.path(), "pub fn x() {}\n");
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity = publish_warm_alpha_population(&req, &tools);
    let before = std::fs::read(req.cache_root.join("population.json")).unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let selective_identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    store_lib_alpha(&req, &tools, &selective_identity);
    let after = std::fs::read(req.cache_root.join("population.json")).unwrap();
    assert_eq!(before, after);
    assert_ne!(
        population_identity.generation_fingerprint,
        selective_identity.generation_fingerprint
    );
}

#[test]
fn selective_generation_entry_hits_on_unchanged_tree() {
    let (req, tools, selective_identity) = published_then_selective_edit();
    store_lib_alpha(&req, &tools, &selective_identity);
    let fingerprint = entry_fingerprint(&selective_identity.input_digest, &req, &tools, "alpha");
    let loaded = crate::rust_cov_cache::load_rust_cov_cache_entry(&req.cache_root, &fingerprint)
        .expect("selective entry");
    assert_eq!(loaded.selector, "alpha");
    assert_eq!(
        loaded.generation_fingerprint,
        selective_identity.generation_fingerprint
    );
}

#[test]
fn consecutive_selective_edits_reuse_complete_population_snapshot() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_crate(repo.path(), "pub fn a() {}\n");
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    publish_derived_state(
        &req,
        &tools,
        &population_identity,
        &["alpha".to_string()],
        false,
    )
    .unwrap();
    let population_generation = population_identity.generation_fingerprint.clone();
    let before = std::fs::read(req.cache_root.join("population.json")).unwrap();

    for body in ["pub fn b() {}\n", "pub fn c() {}\n"] {
        rewrite_lib_and_store_selective(repo.path(), &req, &tools, body);
    }

    let after = std::fs::read(req.cache_root.join("population.json")).unwrap();
    assert_eq!(before, after);
    let manifest = crate::publish_derived::batch_derived_index::read_population_manifest(&req.cache_root)
        .expect("population manifest");
    assert_eq!(manifest.generation_fingerprint, population_generation);
}

#[test]
fn failed_selective_run_skips_prune_and_preserves_population_snapshot() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_crate(repo.path(), "pub fn x() {}\n");
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity = publish_warm_alpha_population(&req, &tools);
    let before_population = std::fs::read(req.cache_root.join("population.json")).unwrap();
    let before_index = std::fs::read(req.cache_root.join("index.json")).unwrap();
    let population_generation = population_identity.generation_fingerprint.clone();
    store_obsolete_entry(
        &req.cache_root,
        "obsolete",
        "obsolete-selective-generation",
        "obsoleteselective01",
    );
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let selective_identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let selective_req = selective_request(&req);

    assert_failed_prune_preserves(
        &selective_req,
        &selective_identity,
        &req.cache_root,
        &before_population,
        &before_index,
        &population_generation,
    );
    assert_success_prune_removes_obsolete(
        &selective_req,
        &selective_identity,
        &req.cache_root,
        &before_population,
    );
}

#[test]
fn consecutive_selective_edits_beyond_retention_keep_only_population_and_current() {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_crate(repo.path(), "pub fn a() {}\n");
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity = publish_warm_alpha_population(&req, &tools);
    let population_generation = population_identity.generation_fingerprint.clone();
    let before = std::fs::read(req.cache_root.join("population.json")).unwrap();

    let mut latest_generation = String::new();
    for (i, body) in [
        "pub fn b() {}\n",
        "pub fn c() {}\n",
        "pub fn d() {}\n",
        "pub fn e() {}\n",
    ]
    .into_iter()
    .enumerate()
    {
        latest_generation =
            rewrite_lib_store_selective_and_prune(repo.path(), &req, &tools, body, i);
    }

    assert_eq!(
        std::fs::read(req.cache_root.join("population.json")).unwrap(),
        before
    );
    assert_eq!(
        entry_generations(&req.cache_root),
        BTreeSet::from([population_generation, latest_generation])
    );
}

fn write_minimal_crate(root: &Path, lib_body: &str) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(root.join("src").join("lib.rs"), lib_body).unwrap();
}

fn store_lib_alpha(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) {
    store_alpha_entry(
        &req.cache_root,
        req,
        tools,
        identity,
        BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
    );
}

fn publish_warm_alpha_population(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> RustCoverageBatchIdentity {
    let population_identity = crate::plan::batch_fingerprint::batch_identity(req, tools).unwrap();
    store_lib_alpha(req, tools, &population_identity);
    publish_derived_state(
        req,
        tools,
        &population_identity,
        &["alpha".to_string()],
        false,
    )
    .unwrap();
    population_identity
}

fn store_obsolete_entry(cache_root: &Path, selector: &str, generation: &str, fingerprint: &str) {
    let obsolete = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        generation,
    );
    store_rust_cov_cache_entry(cache_root, fingerprint, &obsolete).unwrap();
}

fn selective_request(req: &RustCoverageBatchRequest) -> RustCoverageBatchRequest {
    let mut selective_req = req.clone();
    selective_req.population_publication_selectors = None;
    selective_req
}

fn assert_failed_prune_preserves(
    selective_req: &RustCoverageBatchRequest,
    selective_identity: &RustCoverageBatchIdentity,
    cache_root: &Path,
    before_population: &[u8],
    before_index: &[u8],
    population_generation: &str,
) {
    let mut failed = crate::RustCoverageBatchResult {
        completed: Vec::new(),
        counters: crate::RustCoverageBatchCounters::default(),
        batch_error: Some(crate::RustLlvmCovError::InvalidRequest(
            "injected selective failure".to_string(),
        )),
        test_binaries: Vec::new(),
    };
    crate::publish_derived::batch_derived::maybe_prune_obsolete_selective_after_batch(
        selective_req,
        selective_identity,
        &mut failed,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(cache_root.join("population.json")).unwrap(),
        before_population
    );
    assert_eq!(
        std::fs::read(cache_root.join("index.json")).unwrap(),
        before_index
    );
    assert!(
        cache_root
            .join("entries")
            .join("obsoleteselective01.json")
            .is_file()
    );
    let manifest = crate::publish_derived::batch_derived_index::read_population_manifest(cache_root)
        .expect("population manifest");
    assert_eq!(manifest.generation_fingerprint, population_generation);
    assert_eq!(failed.counters.cache_pruned_entries, 0);
}

fn assert_success_prune_removes_obsolete(
    selective_req: &RustCoverageBatchRequest,
    selective_identity: &RustCoverageBatchIdentity,
    cache_root: &Path,
    before_population: &[u8],
) {
    let mut succeeded = crate::RustCoverageBatchResult {
        completed: Vec::new(),
        counters: crate::RustCoverageBatchCounters::default(),
        batch_error: None,
        test_binaries: Vec::new(),
    };
    crate::publish_derived::batch_derived::maybe_prune_obsolete_selective_after_batch(
        selective_req,
        selective_identity,
        &mut succeeded,
    )
    .unwrap();
    assert_eq!(succeeded.counters.cache_pruned_entries, 1);
    assert!(
        !cache_root
            .join("entries")
            .join("obsoleteselective01.json")
            .is_file()
    );
    assert_eq!(
        std::fs::read(cache_root.join("population.json")).unwrap(),
        before_population
    );
}

fn rewrite_lib_and_store_selective(
    root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    body: &str,
) {
    std::fs::write(root.join("src").join("lib.rs"), body).unwrap();
    let selective_identity = crate::plan::batch_fingerprint::batch_identity(req, tools).unwrap();
    store_lib_alpha(req, tools, &selective_identity);
    let _ = crate::publish_derived::batch_derived::prune_obsolete_selective_generations(
        &req.cache_root,
        &selective_identity.generation_fingerprint,
    )
    .unwrap();
}

fn rewrite_lib_store_selective_and_prune(
    root: &Path,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    body: &str,
    obsolete_index: usize,
) -> String {
    std::fs::write(root.join("src").join("lib.rs"), body).unwrap();
    let selective_identity = crate::plan::batch_fingerprint::batch_identity(req, tools).unwrap();
    store_lib_alpha(req, tools, &selective_identity);
    store_obsolete_entry(
        &req.cache_root,
        &format!("obsolete-{obsolete_index}"),
        &format!("obsolete-generation-{obsolete_index}"),
        &format!("obsoleteentry{obsolete_index:02}"),
    );
    let pruned = crate::publish_derived::batch_derived::prune_obsolete_selective_generations(
        &req.cache_root,
        &selective_identity.generation_fingerprint,
    )
    .unwrap();
    assert!(pruned >= 1);
    selective_identity.generation_fingerprint
}

fn entry_generations(cache_root: &Path) -> BTreeSet<String> {
    let mut generations = BTreeSet::new();
    for entry in std::fs::read_dir(cache_root.join("entries")).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        let parsed: RustCovCacheEntry = serde_json::from_slice(&bytes).unwrap();
        generations.insert(parsed.generation_fingerprint);
    }
    generations
}

fn published_then_selective_edit() -> (
    RustCoverageBatchRequest,
    RustCoverageToolIdentity,
    RustCoverageBatchIdentity,
) {
    let repo = tempfile::tempdir().unwrap();
    write_minimal_crate(repo.path(), "pub fn x() {}\n");
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let population_identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    publish_derived_state(
        &req,
        &tools,
        &population_identity,
        &["alpha".to_string()],
        false,
    )
    .unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let selective_identity = crate::plan::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    (req, tools, selective_identity)
}
