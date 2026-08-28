use std::fs;
use std::path::{Path, PathBuf};

use kiss::check_universe_cache::CachedLineCoverageRecord;
use serde::{Deserialize, Serialize};

use crate::analyze::line_coverage::{
    LineCoverageRecord, cached_line_records, line_records_from_cache,
};
use crate::analyze_cache::{fnv1a64, mix_sorted_paths_len_mtime};
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;

const SCHEMA_VERSION: &str = "kiss-cov-records-v6";

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

pub(crate) fn try_load_cov_records(
    key: &CovRecordsCacheKey<'_>,
) -> Option<Vec<LineCoverageRecord>> {
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
    h = fnv1a64(
        h,
        &[u8::from(key.required.python), u8::from(key.required.rust)],
    );
    if let Some(lang) = key.lang_filter {
        h = fnv1a64(h, lang.as_bytes());
    }
    for prefix in key.ignore {
        h = fnv1a64(h, prefix.as_bytes());
        h = fnv1a64(h, &[0]);
    }
    h = mix_sorted_paths_len_mtime(h, key.py_files);
    h = mix_sorted_paths_len_mtime(h, key.rs_files);
    let cargo = key.repo_root.join("Cargo.toml");
    h = fnv1a64(h, cargo.to_string_lossy().as_bytes());
    if let Ok(bytes) = fs::read(&cargo) {
        h = fnv1a64(h, &bytes);
    }
    if key.required.python {
        h = fnv1a64(h, python_backend_identity(key.repo_root)?.as_bytes());
    }
    if key.required.rust {
        h = fnv1a64(h, rust_backend_identity(key.repo_root)?.as_bytes());
    }
    Some(format!("{h:016x}"))
}

fn python_backend_identity(repo_root: &Path) -> Option<String> {
    if let Ok(pinned) =
        crate::test_runner::python_coverage_index::try_load_pinned_python_generation_warm(repo_root)
    {
        return Some(format!(
            "py-gen:{}:{}",
            pinned.generation_id, pinned.plan.base_identity.input_fingerprint
        ));
    }
    let path = python_coverage_cache_root_population(repo_root)?;
    let bytes = fs::read(path).ok()?;

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

fn python_coverage_cache_root_population(repo_root: &Path) -> Option<PathBuf> {
    let cache =
        crate::test_runner::python_coverage_index::python_coverage_cache_root(repo_root).ok()?;
    let candidate = cache.join("population.json");
    candidate.is_file().then_some(candidate)
}

fn rust_backend_identity(repo_root: &Path) -> Option<String> {
    let cache = repo_root.join(".kiss").join("rust_llvm_cov_cache");
    let witness = cache.join("execution_witness.json");
    if let Some(stamp) = file_len_mtime_stamp(&witness) {
        return Some(format!("rs-wit-file:{stamp}"));
    }
    if let Some(identity) = rust_check_aggregate_backend_identity(&cache) {
        return Some(identity);
    }
    rust_population_backend_identity(&cache)
}

fn file_len_mtime_stamp(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}:{mtime}", meta.len()))
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
