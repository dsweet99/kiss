use super::*;
use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::rust_cov_cache::store_rust_cov_cache_entry;
use crate::rust_llvm_cov_runner::{RustCovCacheStatus, RustLlvmCovOutcome};
use std::time::Duration;

#[test]
fn load_manifest_generation_entries_ignores_non_json_and_stale_entries() {
    let repo = snapshot_repo();
    let cache = repo.path().join(".kiss/rust_llvm_cov_cache");
    std::fs::create_dir_all(cache.join("entries")).unwrap();
    std::fs::write(cache.join("entries/readme.txt"), "ignored").unwrap();
    store_entry(
        &cache,
        "stale",
        entry(
            "alpha",
            "old-generation",
            TestStatus::Passed,
            "src/lib.rs",
            99,
        ),
    );
    store_entry(
        &cache,
        "alpha",
        entry("alpha", "generation", TestStatus::Passed, "src/lib.rs", 1),
    );
    store_entry(
        &cache,
        "extra",
        entry("gamma", "generation", TestStatus::Passed, "src/lib.rs", 7),
    );

    let entries =
        load_manifest_generation_entries(&cache, repo.path(), &population(&["alpha"])).unwrap();

    assert_eq!(entries["src/lib.rs"], BTreeSet::from([1]));
}

#[test]
fn load_manifest_generation_entries_rejects_invalid_current_entries() {
    assert_rejects_entry(entry(
        "alpha",
        "generation",
        TestStatus::Failed,
        "src/lib.rs",
        1,
    ));

    let repo = snapshot_repo();
    let cache = repo.path().join(".kiss/rust_llvm_cov_cache");
    store_entry(
        &cache,
        "alpha-a",
        entry("alpha", "generation", TestStatus::Passed, "src/lib.rs", 1),
    );
    store_entry(
        &cache,
        "alpha-b",
        entry("alpha", "generation", TestStatus::Passed, "src/lib.rs", 2),
    );
    assert!(
        load_manifest_generation_entries(&cache, repo.path(), &population(&["alpha"])).is_none()
    );

    assert_rejects_entry(entry(
        "alpha",
        "generation",
        TestStatus::Passed,
        "../outside.rs",
        1,
    ));
}

#[test]
fn load_manifest_generation_entries_requires_exact_selector_population() {
    let repo = snapshot_repo();
    let cache = repo.path().join(".kiss/rust_llvm_cov_cache");
    store_entry(
        &cache,
        "alpha",
        entry("alpha", "generation", TestStatus::Passed, "src/lib.rs", 1),
    );

    assert!(
        load_manifest_generation_entries(&cache, repo.path(), &population(&["alpha", "beta"]))
            .is_none()
    );
}

fn assert_rejects_entry(entry: RustCovCacheEntry) {
    let repo = snapshot_repo();
    let cache = repo.path().join(".kiss/rust_llvm_cov_cache");
    store_entry(&cache, "alpha", entry);
    assert!(
        load_manifest_generation_entries(&cache, repo.path(), &population(&["alpha"])).is_none()
    );
}

fn snapshot_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    repo
}

fn population(selectors: &[&str]) -> RustPopulationState {
    RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: selectors
            .iter()
            .map(|selector| (*selector).to_string())
            .collect(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    }
}

fn entry(
    selector: &str,
    generation: &str,
    status: TestStatus,
    file: &str,
    line: u32,
) -> RustCovCacheEntry {
    RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: selector.to_string(),
            status,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage {
                files: BTreeMap::from([(file.to_string(), BTreeSet::from([line]))]),
            },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        generation,
    )
}

fn store_entry(cache: &Path, fingerprint: &str, entry: RustCovCacheEntry) {
    store_rust_cov_cache_entry(cache, fingerprint, &entry).unwrap();
}
