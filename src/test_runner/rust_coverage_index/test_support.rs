use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::{CACHE_SCHEMA_VERSION, rust_coverage_cache_root};

pub(super) fn test_generation_fingerprint(repo_root: &Path) -> String {
    super::current_rust_coverage_batch_identity(repo_root, &[])
        .expect("test batch identity")
        .generation_fingerprint
}

pub(crate) fn test_entries_fingerprint(repo_root: &Path, test_args: &[String]) -> String {
    let cache_root = rust_coverage_cache_root(repo_root);
    let generation = super::current_rust_coverage_batch_identity(repo_root, test_args)
        .expect("test batch identity")
        .generation_fingerprint;
    rust_llvm_cov_runner::generation_entries_fingerprint(&cache_root, &generation)
        .expect("test entries fingerprint")
}

pub(crate) fn write_test_entry(
    repo_root: &Path,
    name: &str,
    selector: &str,
    status: TestStatus,
    coverage: RustLineCoverage,
) {
    write_test_entry_with_args(repo_root, name, selector, status, coverage, &[]);
}

pub(crate) fn write_test_entry_with_args(
    repo_root: &Path,
    name: &str,
    selector: &str,
    status: TestStatus,
    coverage: RustLineCoverage,
    test_args: &[String],
) {
    let path = rust_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let generation = super::current_rust_coverage_batch_identity(repo_root, test_args)
        .expect("test batch identity")
        .generation_fingerprint;
    let entry = serde_json::json!({
        "schema_version": CACHE_SCHEMA_VERSION,
        "generation_fingerprint": generation,
        "selector": selector,
        "status": status,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}
