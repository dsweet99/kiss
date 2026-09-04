use std::path::Path;

use crate::rust_llvm_cov_runner::publish_derived::batch_derived::derived_generation_line_index;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::read_population_manifest;

#[test]
fn real_kiss_cache_index_matches_derived_entries_when_env_set() {
    if std::env::var_os("KISS_REUSABLE_PRIOR_REAL_CACHE").is_none() {
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    let manifest = read_population_manifest(&cache).expect("population manifest");
    let index_bytes = std::fs::read(cache.join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_slice(&index_bytes).unwrap();
    let derived = derived_generation_line_index(&cache, repo, &manifest.generation_fingerprint)
        .expect("derived index");
    let stored: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        serde_json::from_value(index["files"].clone()).expect("files map");
    assert_eq!(derived, stored, "stored index must match derived entries");
}

#[test]
fn real_kiss_cache_load_current_when_env_set() {
    if std::env::var_os("KISS_REUSABLE_PRIOR_REAL_CACHE").is_none() {
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    let seal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("input_mtime_seal.json")).unwrap())
            .unwrap();
    let pop: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("population.json")).unwrap()).unwrap();
    let selectors: Vec<String> = pop["selectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect();
    let ordinary = seal["ordinary_source_digests"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    let identity = crate::rust_llvm_cov_runner::RustCoverageBatchIdentity {
        input_digest: seal["input_digest"].as_str().unwrap().to_string(),
        generation_fingerprint: seal["generation_fingerprint"].as_str().unwrap().to_string(),
        selection_context_fingerprint: seal["selection_context_fingerprint"]
            .as_str()
            .unwrap()
            .to_string(),
        ordinary_source_digests: ordinary,
    };
    let loaded = crate::rust_llvm_cov_runner::load_current_population_state(
        &cache,
        repo,
        &identity,
        Some(&selectors),
    );
    assert!(
        loaded.is_some(),
        "load_current with seal identity and pop selectors must succeed"
    );
}
