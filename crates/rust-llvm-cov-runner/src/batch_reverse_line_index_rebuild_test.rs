use super::*;
use std::path::PathBuf;

/// Rebuild reverse index for the workspace cache (manual priming).
#[test]
#[ignore = "rewrites local .kiss/rust_llvm_cov_cache reverse index"]
fn rebuild_workspace_reverse_line_index() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    let population: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("population.json")).unwrap()).unwrap();
    let generation = population["generation_fingerprint"].as_str().unwrap();
    let entries_fp = population["entries_fingerprint"].as_str().unwrap();
    publish_reverse_line_index(&cache, &repo, generation, entries_fp).unwrap();
    assert!(reverse_line_index_dir(&cache).join("meta.json").is_file());
}
