use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use serde::{Deserialize, Serialize};

pub(crate) const CACHE_SCHEMA_VERSION: &str = "rust-llvm-cov-cache-v1";
pub(crate) const INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v1";

pub(crate) type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn rust_coverage_index_path(repo_root: &Path) -> PathBuf {
    rust_coverage_cache_root(repo_root).join("index.json")
}

pub(crate) fn rust_coverage_cache_root(repo_root: &Path) -> PathBuf {
    repo_root.join(".kiss").join("rust_llvm_cov_cache")
}

pub(crate) fn rebuild_rust_coverage_index(repo_root: &Path) -> Result<RustCoverageIndex, String> {
    let index = build_rust_coverage_index(repo_root)?;
    write_rust_coverage_index(repo_root, &index)?;
    Ok(index)
}

pub(crate) fn load_current_rust_coverage_index(repo_root: &Path) -> Option<RustCoverageIndex> {
    #[derive(Deserialize)]
    struct OnDiskIndex {
        schema_version: String,
        source_root: String,
        entries_fingerprint: String,
        files: RustCoverageIndex,
    }

    let path = rust_coverage_index_path(repo_root);
    let bytes = fs::read(path).ok()?;
    let index: OnDiskIndex = serde_json::from_slice(&bytes).ok()?;
    if index.schema_version != INDEX_SCHEMA_VERSION {
        return None;
    }
    if index.source_root != normalized_repo_root(repo_root) {
        return None;
    }
    let current_fingerprint = entries_fingerprint(&rust_coverage_cache_root(repo_root)).ok()?;
    (index.entries_fingerprint == current_fingerprint).then_some(index.files)
}

pub(crate) fn select_rust_source_selectors_from_index(
    repo_root: &Path,
    source_paths: &[PathBuf],
) -> Option<BTreeSet<String>> {
    if source_paths.is_empty() {
        return Some(BTreeSet::new());
    }
    let index = load_current_rust_coverage_index(repo_root)?;
    selectors_for_source_paths(repo_root, source_paths, &index)
}

pub(crate) fn selectors_for_source_paths(
    repo_root: &Path,
    source_paths: &[PathBuf],
    index: &RustCoverageIndex,
) -> Option<BTreeSet<String>> {
    let mut selectors = BTreeSet::new();
    for source_path in source_paths {
        let rel = repo_relative_path(repo_root, source_path)?;
        let file_selectors = index.get(&rel)?;
        if file_selectors.is_empty() {
            return None;
        }
        selectors.extend(file_selectors.iter().cloned());
    }
    Some(selectors)
}

fn build_rust_coverage_index(repo_root: &Path) -> Result<RustCoverageIndex, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let mut files: RustCoverageIndex = BTreeMap::new();
    for entry_path in rust_coverage_entry_paths(&cache_root) {
        let Some((selector, status, coverage)) = load_entry_for_index(&entry_path) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            if let Some(rel) = repo_relative_coverage_file(repo_root, file) {
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    Ok(files)
}

fn rust_coverage_entry_paths(cache_root: &Path) -> Vec<PathBuf> {
    let entries_dir = cache_root.join("entries");
    let Ok(entries) = fs::read_dir(entries_dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    paths.sort();
    paths
}

fn load_entry_for_index(path: &Path) -> Option<(String, TestStatus, RustLineCoverage)> {
    #[derive(Deserialize)]
    struct RustCovCacheEntryForIndex {
        schema_version: String,
        selector: String,
        status: TestStatus,
        coverage: RustLineCoverage,
    }

    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntryForIndex = serde_json::from_slice(&bytes).ok()?;
    (entry.schema_version == CACHE_SCHEMA_VERSION).then_some((
        entry.selector,
        entry.status,
        entry.coverage,
    ))
}

fn write_rust_coverage_index(repo_root: &Path, index: &RustCoverageIndex) -> Result<(), String> {
    #[derive(Serialize)]
    struct OnDiskIndex<'a> {
        schema_version: &'a str,
        source_root: String,
        entries_fingerprint: String,
        files: &'a RustCoverageIndex,
    }

    let path = rust_coverage_index_path(repo_root);
    let parent = path
        .parent()
        .ok_or_else(|| "error: kiss test: Rust coverage index path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp_path = parent.join(format!(".index.{}.tmp", unique_suffix()));
    let mut file = create_new_file(&tmp_path).map_err(|e| e.to_string())?;
    let payload = OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        source_root: normalized_repo_root(repo_root),
        entries_fingerprint: entries_fingerprint(&rust_coverage_cache_root(repo_root))
            .map_err(|e| e.to_string())?,
        files: index,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(&tmp_path, path).map_err(|e| e.to_string())
}

pub(crate) fn create_new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

pub(crate) fn repo_relative_coverage_file(repo_root: &Path, file: &str) -> Option<String> {
    repo_relative_path(repo_root, Path::new(file))
}

pub(crate) fn repo_relative_path(repo_root: &Path, path: &Path) -> Option<String> {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let candidate = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let joined = root.join(path);
        joined.canonicalize().unwrap_or(joined)
    };
    let rel = candidate.strip_prefix(&root).ok()?;
    if rel.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn normalized_repo_root(repo_root: &Path) -> String {
    repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn entries_fingerprint(cache_root: &Path) -> io::Result<String> {
    let mut h = 0xcbf2_9ce4_8422_2325;
    let update = |mut h: u64, bytes: &[u8]| {
        const PRIME: u64 = 0x0100_0000_01b3;
        for byte in bytes {
            h = (h ^ u64::from(*byte)).wrapping_mul(PRIME);
        }
        h
    };
    h = update(h, CACHE_SCHEMA_VERSION.as_bytes());
    for path in rust_coverage_entry_paths(cache_root) {
        let meta = fs::metadata(&path)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        h = update(h, name.as_bytes());
        h = update(h, &[0]);
        h = update(h, meta.len().to_string().as_bytes());
        h = update(h, &[0]);
        if let Ok(modified) = meta.modified()
            && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
        {
            h = update(h, duration.as_nanos().to_string().as_bytes());
        }
        h = update(h, &[0]);
    }
    Ok(format!("{h:016x}"))
}

pub(crate) fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}.{}", process::id(), nanos)
}

#[cfg(test)]
#[path = "rust_coverage_index_test.rs"]
mod tests;
