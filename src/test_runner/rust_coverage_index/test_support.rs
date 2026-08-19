use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use super::{CACHE_SCHEMA_VERSION, RustCoverageIndex, rust_coverage_cache_root};

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
        "test_binary_ids": ["test-bin"],
    });
    fs::write(path, serde_json::to_vec(&entry).unwrap()).unwrap();
    rust_llvm_cov_runner::invalidate_entry_state(&rust_coverage_cache_root(repo_root));
}

pub(crate) fn write_rust_population_manifest_for_args(
    repo_root: &Path,
    selectors: &[String],
    test_args: &[String],
) -> Result<(), String> {
    let (mut req, tools) = super::resolved_rust_batch_request_parts(repo_root, test_args)?;
    let mut selectors = selectors.to_vec();
    selectors.sort();
    selectors.dedup();
    req.logical_selectors = selectors.clone();
    req.population_publication_selectors = Some(selectors.clone());
    let identity = rust_llvm_cov_runner::batch_identity(&req, &tools)
        .map_err(|err| format!("batch identity: {err}"))?;
    rust_llvm_cov_runner::publish_derived_state(&req, &tools, &identity, &selectors, true)
        .map_err(|err| format!("{err:?}"))?;
    Ok(())
}

pub(crate) fn write_rust_coverage_index(
    repo_root: &Path,
    index: &RustCoverageIndex,
) -> Result<(), String> {
    let selectors = index
        .values()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    write_rust_population_manifest_for_args(repo_root, &selectors, &[])
}

pub(crate) fn rebuild_rust_coverage_index(repo_root: &Path) -> Result<RustCoverageIndex, String> {
    let index = super::build_rust_coverage_index_with_filter(repo_root, |path, repo_root| {
        rust_llvm_cov_runner::repo_relative_coverage_file(repo_root, &path.to_string_lossy())
            .is_some()
    })?;
    write_rust_coverage_index(repo_root, &index)?;
    Ok(index)
}

pub(crate) fn load_current_rust_coverage_index(
    repo_root: &Path,
    test_args: &[String],
) -> Option<RustCoverageIndex> {
    super::load_current_rust_population_state(repo_root, None, test_args).map(|state| state.line_index)
}

pub(crate) fn rust_coverage_index_path(repo_root: &Path) -> std::path::PathBuf {
    rust_coverage_cache_root(repo_root).join("index.json")
}

pub(crate) fn rust_population_manifest_path(repo_root: &Path) -> std::path::PathBuf {
    rust_coverage_cache_root(repo_root).join("population.json")
}

pub(crate) fn normalized_repo_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}
