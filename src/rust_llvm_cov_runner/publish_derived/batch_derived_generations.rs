use crate::rust_llvm_cov_runner::rust_cov_cache::{RustCovCacheEntry, repo_relative_coverage_file};
use crate::rust_llvm_cov_runner::{RustLineCoverage, RustLlvmCovError};
use crate::rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use super::batch_io_skip_not_found::{
    dir_entry_path_ok_missing, read_dir_ok_missing, read_ok_missing,
};

type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn count_generations(cache_root: &Path) -> Result<usize, RustLlvmCovError> {
    let Some(entries) =
        read_dir_ok_missing(&cache_root.join("entries")).map_err(RustLlvmCovError::Io)?
    else {
        return Ok(0);
    };
    let mut generations = BTreeSet::new();
    for entry in entries {
        let Some(path) = dir_entry_path_ok_missing(entry).map_err(RustLlvmCovError::Io)? else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(bytes) = read_ok_missing(&path).map_err(RustLlvmCovError::Io)? else {
            continue;
        };
        let Ok(parsed): Result<RustCovCacheEntry, _> = serde_json::from_slice(&bytes) else {
            continue;
        };
        if !parsed.generation_fingerprint.is_empty() {
            generations.insert(parsed.generation_fingerprint);
        }
    }
    Ok(generations.len())
}

pub(crate) fn build_generation_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<RustCoverageIndex, RustLlvmCovError> {
    let mut files: RustCoverageIndex = BTreeMap::new();
    let Some(entries) =
        read_dir_ok_missing(&cache_root.join("entries")).map_err(RustLlvmCovError::Io)?
    else {
        return Ok(files);
    };
    for entry in entries {
        let Some(path) = dir_entry_path_ok_missing(entry).map_err(RustLlvmCovError::Io)? else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some((selector, status, coverage)) = load_index_entry(&path, generation) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            if let Some(rel) = repo_relative_coverage_file(source_root, file) {
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    Ok(files)
}

fn load_index_entry(
    path: &Path,
    generation: &str,
) -> Option<(String, TestStatus, RustLineCoverage)> {
    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntry = serde_json::from_slice(&bytes).ok()?;
    if entry.generation_fingerprint != generation {
        return None;
    }
    Some((entry.selector, entry.status, entry.coverage))
}
