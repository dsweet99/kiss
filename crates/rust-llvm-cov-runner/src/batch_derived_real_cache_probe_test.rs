use std::path::Path;

use crate::batch_derived::derived_generation_line_index;
use crate::batch_derived_index::read_population_manifest;

#[test]
fn real_kiss_cache_index_matches_derived_entries_when_env_set() {
    if std::env::var_os("KISS_REUSABLE_PRIOR_REAL_CACHE").is_none() {
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    let manifest = read_population_manifest(&cache).expect("population manifest");
    let index_bytes = std::fs::read(cache.join("index.json")).unwrap();
    let index: serde_json::Value = serde_json::from_slice(&index_bytes).unwrap();
    let derived = derived_generation_line_index(
        &cache,
        &repo,
        &manifest.generation_fingerprint,
    )
    .expect("derived index");
    let stored: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        serde_json::from_value(index["files"].clone()).expect("files map");
    assert_eq!(derived, stored, "stored index must match derived entries");
}
