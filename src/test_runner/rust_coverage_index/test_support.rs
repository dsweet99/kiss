use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::{CACHE_SCHEMA_VERSION, rust_coverage_cache_root};

pub(super) fn write_test_entry(
    repo_root: &Path,
    name: &str,
    selector: &str,
    status: TestStatus,
    coverage: RustLineCoverage,
) {
    let path = rust_coverage_cache_root(repo_root)
        .join("entries")
        .join(format!("{name}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": CACHE_SCHEMA_VERSION,
        "selector": selector,
        "status": status,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": coverage,
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
}
