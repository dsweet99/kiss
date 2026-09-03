use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use kiss::check_universe_cache::CachedLineCoverageRecord;
use serde::{Deserialize, Serialize};

use crate::analyze::line_coverage::{
    LineCoverageRecord, cached_line_records, line_records_from_cache,
};
use crate::analyze_cache::fnv1a64;
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;

const SCHEMA_VERSION: &str = "kiss-cov-records-v10";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CovRecordsCache {
    schema_version: String,
    fingerprint: String,
    records: Vec<CachedLineCoverageRecord>,
    #[serde(default)]
    orphan_clean_policy: String,
    #[serde(default)]
    orphan_clean_records_digest: String,
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
    pub(crate) pytest_args: &'a [String],
}

pub(crate) fn try_load_cov_records(
    key: &CovRecordsCacheKey<'_>,
) -> Option<Vec<LineCoverageRecord>> {
    try_load_cov_records_with_orphan_state(key).map(|(records, _)| records)
}

pub(crate) fn try_load_cov_records_with_orphan_state(
    key: &CovRecordsCacheKey<'_>,
) -> Option<(Vec<LineCoverageRecord>, String)> {
    let fingerprint = cov_records_fingerprint(key)?;
    let raw = fs::read(cache_path(key.repo_root)).ok()?;
    let cache: CovRecordsCache = serde_json::from_slice(&raw).ok()?;
    if cache.schema_version != SCHEMA_VERSION || cache.fingerprint != fingerprint {
        return None;
    }
    let orphan_clean_policy = if cache.orphan_clean_records_digest == records_digest(&cache.records)
    {
        cache.orphan_clean_policy
    } else {
        String::default()
    };
    Some((line_records_from_cache(&cache.records), orphan_clean_policy))
}

pub(crate) fn store_cov_records(key: &CovRecordsCacheKey<'_>, records: &[LineCoverageRecord]) {
    let Some(fingerprint) = cov_records_fingerprint(key) else {
        return;
    };
    let Some(_lock) = lock_cache(key.repo_root) else {
        return;
    };
    let path = cache_path(key.repo_root);
    let cached_records = cached_line_records(records);
    let new_digest = records_digest(&cached_records);
    let preserved = load_cache(&path).filter(|cache| {
        cache.schema_version == SCHEMA_VERSION
            && cache.fingerprint == fingerprint
            && cache.orphan_clean_records_digest == new_digest
            && records_digest(&cache.records) == new_digest
    });
    let cache = CovRecordsCache {
        schema_version: SCHEMA_VERSION.to_string(),
        fingerprint,
        records: cached_records,
        orphan_clean_policy: preserved
            .as_ref()
            .map(|cache| cache.orphan_clean_policy.clone())
            .unwrap_or_default(),
        orphan_clean_records_digest: preserved
            .map(|cache| cache.orphan_clean_records_digest)
            .unwrap_or_default(),
    };
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    publish_cache_bytes(&path, &bytes);
}

pub(crate) fn mark_cached_records_orphan_clean(key: &CovRecordsCacheKey<'_>, policy: &str) {
    let Some(fingerprint) = cov_records_fingerprint(key) else {
        return;
    };
    let path = cache_path(key.repo_root);
    let Some(_lock) = lock_cache(key.repo_root) else {
        return;
    };
    let Some(mut cache) = load_cache(&path) else {
        return;
    };
    if cache.schema_version != SCHEMA_VERSION || cache.fingerprint != fingerprint {
        return;
    }
    cache.orphan_clean_policy = policy.to_string();
    cache.orphan_clean_records_digest = records_digest(&cache.records);
    let Ok(bytes) = serde_json::to_vec(&cache) else {
        return;
    };
    publish_cache_bytes(&path, &bytes);
}

fn lock_cache(repo_root: &Path) -> Option<fs::File> {
    lock_cache_for(repo_root, Duration::from_secs(30))
}

fn lock_cache_for(repo_root: &Path, timeout: Duration) -> Option<fs::File> {
    let dir = repo_root.join(".kiss");
    fs::create_dir_all(&dir).ok()?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("cov_records_cache.lock"))
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Some(file),
            Err(err) if err.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

fn load_cache(path: &Path) -> Option<CovRecordsCache> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn records_digest(records: &[CachedLineCoverageRecord]) -> String {
    serde_json::to_vec(records)
        .map(|bytes| format!("{:016x}", fnv1a64(0xcbf2_9ce4_8422_2325, &bytes)))
        .unwrap_or_default()
}

fn publish_cache_bytes(path: &Path, bytes: &[u8]) {
    let tmp = path.with_file_name(format!(
        ".cov_records.{}.tmp",
        kiss::kiss_publication_barrier::unique_process_suffix()
    ));
    if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, path).is_ok() {
        return;
    }
    let _ = fs::remove_file(tmp);
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
    h = mix_sorted_source_contents(h, key.py_files.iter().chain(key.rs_files))?;
    let cargo = key.repo_root.join("Cargo.toml");
    h = fnv1a64(h, cargo.to_string_lossy().as_bytes());
    if let Ok(bytes) = fs::read(&cargo) {
        h = fnv1a64(h, &bytes);
    }
    if key.required.python {
        let identity =
            crate::test_runner::python_coverage_index::current_python_execution_identity(
                key.repo_root,
                key.pytest_args,
            )
            .ok()?;
        h = fnv1a64(h, &serde_json::to_vec(&identity).ok()?);
        h = fnv1a64(h, python_backend_identity(key.repo_root)?.as_bytes());
    }
    if key.required.rust {
        h = fnv1a64(h, rust_backend_identity(key.repo_root)?.as_bytes());
    }
    Some(format!("{h:016x}"))
}

fn mix_sorted_source_contents<'a, I>(mut h: u64, files: I) -> Option<u64>
where
    I: IntoIterator<Item = &'a PathBuf>,
{
    let mut paths: Vec<&PathBuf> = files.into_iter().collect();
    paths.sort();
    for path in paths {
        h = fnv1a64(h, path.to_string_lossy().as_bytes());
        h = fnv1a64(h, &[0]);
        h = fnv1a64(h, &fs::read(path).ok()?);
        h = fnv1a64(h, &[0]);
    }
    Some(h)
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
    let mut parts = Vec::new();
    if let Some(identity) = file_content_identity(&witness) {
        parts.push(format!("wit:{identity}"));
    }
    if let Some(identity) = file_content_identity(&cache.join("current_generation.json")) {
        parts.push(format!("gen:{identity}"));
    }
    if let Some(identity) = rust_check_aggregate_backend_identity(&cache) {
        parts.push(identity);
    }
    if let Some(identity) = rust_population_backend_identity(&cache) {
        parts.push(identity);
    }
    (!parts.is_empty()).then(|| format!("rs:{}", parts.join("|")))
}

fn file_content_identity(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!(
        "{}:{:016x}",
        bytes.len(),
        fnv1a64(0xcbf2_9ce4_8422_2325, &bytes)
    ))
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
