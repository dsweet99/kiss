//! Warm-path cache of per-file line-coverage records for `kiss cov`.
//!
//! Invalidation keys on source mtimes/sizes plus lightweight on-disk coverage
//! backend identities (Python population + Rust check_aggregate fingerprints),
//! so a warm hit skips snapshot reload and record recomputation.

use std::fs;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedLineCoverageRecord;
use serde::{Deserialize, Serialize};

use crate::analyze::line_coverage::{
    LineCoverageRecord, cached_line_records, line_records_from_cache,
};
use crate::analyze_cache::{fnv1a64, mix_sorted_paths_len_mtime};
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;

const SCHEMA_VERSION: &str = "kiss-cov-records-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CovRecordsCache {
    schema_version: String,
    fingerprint: String,
    records: Vec<CachedLineCoverageRecord>,
}

#[derive(Clone, Debug)]
pub(crate) struct CovRecordsCacheKey<'a> {
    pub(crate) repo_root: &'a Path,
    pub(crate) py_files: &'a [PathBuf],
    pub(crate) rs_files: &'a [PathBuf],
    pub(crate) required: RequiredCoverageLanguages,
    pub(crate) threshold: usize,
    pub(crate) bypass_gate: bool,
    pub(crate) ignore: &'a [String],
    pub(crate) lang_filter: Option<&'a str>,
}

pub(crate) fn try_load_cov_records(key: &CovRecordsCacheKey<'_>) -> Option<Vec<LineCoverageRecord>> {
    let fingerprint = cov_records_fingerprint(key)?;
    let raw = fs::read(cache_path(key.repo_root)).ok()?;
    let cache: CovRecordsCache = serde_json::from_slice(&raw).ok()?;
    if cache.schema_version != SCHEMA_VERSION || cache.fingerprint != fingerprint {
        return None;
    }
    Some(line_records_from_cache(&cache.records))
}

pub(crate) fn store_cov_records(key: &CovRecordsCacheKey<'_>, records: &[LineCoverageRecord]) {
    let Some(fingerprint) = cov_records_fingerprint(key) else {
        return;
    };
    let cache = CovRecordsCache {
        schema_version: SCHEMA_VERSION.to_string(),
        fingerprint,
        records: cached_line_records(records),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    let path = cache_path(key.repo_root);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

fn cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("cov_records_cache.json")
}

fn cov_records_fingerprint(key: &CovRecordsCacheKey<'_>) -> Option<String> {
    let mut h = fnv1a64(0xcbf2_9ce4_8422_2325, SCHEMA_VERSION.as_bytes());
    h = fnv1a64(h, &key.threshold.to_le_bytes());
    h = fnv1a64(h, &[u8::from(key.bypass_gate)]);
    h = fnv1a64(h, &[u8::from(key.required.python), u8::from(key.required.rust)]);
    if let Some(lang) = key.lang_filter {
        h = fnv1a64(h, lang.as_bytes());
    }
    for prefix in key.ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    h = mix_sorted_paths_len_mtime(h, key.py_files);
    h = mix_sorted_paths_len_mtime(h, key.rs_files);
    if key.required.python {
        h = fnv1a64(h, python_backend_identity(key.repo_root)?.as_bytes());
    }
    if key.required.rust {
        h = fnv1a64(h, rust_backend_identity(key.repo_root)?.as_bytes());
    }
    Some(format!("{h:016x}"))
}

pub(crate) fn python_backend_identity_for_file_list(repo_root: &Path) -> Option<String> {
    python_backend_identity(repo_root)
}

fn python_backend_identity(repo_root: &Path) -> Option<String> {
    let path = find_python_population_manifest(repo_root)?;
    let bytes = fs::read(path).ok()?;
    // Ignore selectors: counting them forces a full array parse of a large manifest.
    #[derive(serde::Deserialize)]
    struct PopHead {
        input_fingerprint: String,
        entries_fingerprint: String,
    }
    let value: PopHead = serde_json::from_slice(&bytes).ok()?;
    Some(format!(
        "py:{}:{}",
        value.input_fingerprint, value.entries_fingerprint
    ))
}

fn find_python_population_manifest(repo_root: &Path) -> Option<PathBuf> {
    let hosts = repo_root.join(".kiss/rslip_cache/hosts");
    let host_dirs = fs::read_dir(hosts).ok()?;
    for entry in host_dirs.flatten() {
        let candidate = entry.path().join("population.json");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn rust_backend_identity_for_file_list(repo_root: &Path) -> Option<String> {
    rust_backend_identity(repo_root)
}

fn rust_backend_identity(repo_root: &Path) -> Option<String> {
    let cache = repo_root.join(".kiss").join("rust_llvm_cov_cache");
    if let Some(identity) = rust_check_aggregate_backend_identity(&cache) {
        return Some(identity);
    }
    rust_population_backend_identity(&cache)
}

fn rust_check_aggregate_backend_identity(cache: &Path) -> Option<String> {
    let bytes = fs::read(cache.join("check_aggregate.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let integrity = value.get("integrity_fingerprint")?.as_str()?;
    let input = value.get("input_fingerprint")?.as_str()?;
    let generation = value.get("generation_fingerprint")?.as_str()?;
    Some(format!("rs-agg:{integrity}:{input}:{generation}"))
}

fn rust_population_backend_identity(cache: &Path) -> Option<String> {
    let bytes = fs::read(cache.join("population.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let input = value.get("input_fingerprint")?.as_str()?;
    let generation = value.get("generation_fingerprint")?.as_str()?;
    let entries = value.get("entries_fingerprint")?.as_str()?;
    let n_selectors = value.get("selectors")?.as_array()?.len();
    Some(format!(
        "rs-pop:{input}:{generation}:{entries}:{n_selectors}"
    ))
}

#[cfg(test)]
#[path = "cov_records_cache_test.rs"]
mod tests;
