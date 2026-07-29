//! Generation-scoped derived reverse line index for fast PATH::symbol selection.
//!
//! Authoritative coverage remains per-test entry JSON. This index is disposable
//! and rebuildable from those entries.

use crate::rust_cov_cache::{
    RustCovCacheEntry, create_new_cache_file, repo_relative_coverage_file, rust_cov_unique_suffix,
};
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError};
use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const REVERSE_LINE_INDEX_SCHEMA: &str = "rust-llvm-cov-reverse-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ReverseMeta {
    schema_version: String,
    generation_fingerprint: String,
    entries_fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileReverseRecord {
    file: String,
    /// Contiguous covered ranges with covering selector IDs (into `selectors.json`).
    ranges: Vec<(u32, u32, Vec<u32>)>,
}

pub fn reverse_line_index_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("reverse_line_index")
}

pub fn publish_reverse_line_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
) -> Result<(), RustLlvmCovError> {
    let built = build_reverse_line_index(cache_root, source_root, generation)?;
    write_reverse_line_index(cache_root, generation, entries_fingerprint, &built)
}

pub fn query_reverse_line_index(
    cache_root: &Path,
    generation: &str,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    if changed_rels.is_empty() {
        return Some(BTreeMap::new());
    }
    let root = reverse_line_index_dir(cache_root);
    let meta: ReverseMeta = serde_json::from_slice(&fs::read(root.join("meta.json")).ok()?).ok()?;
    if meta.schema_version != REVERSE_LINE_INDEX_SCHEMA
        || meta.generation_fingerprint != generation
    {
        return None;
    }
    let selectors: Vec<String> =
        serde_json::from_slice(&fs::read(root.join("selectors.json")).ok()?).ok()?;
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (rel, wanted) in changed_rels {
        if wanted.is_empty() {
            continue;
        }
        let Some(record) = load_file_record(&root, rel) else {
            continue;
        };
        if record.file != *rel {
            continue;
        }
        let mut selected = BTreeSet::new();
        for (start, end, ids) in &record.ranges {
            if range_overlaps_wanted(*start, *end, wanted) {
                for id in ids {
                    if let Some(sel) = selectors.get(*id as usize) {
                        selected.insert(sel.clone());
                    }
                }
            }
        }
        if !selected.is_empty() {
            out.insert(rel.clone(), selected);
        }
    }
    Some(out)
}

fn range_overlaps_wanted(start: u32, end: u32, wanted: &BTreeSet<u32>) -> bool {
    wanted.range(start..=end).next().is_some()
}

fn load_file_record(root: &Path, rel: &str) -> Option<FileReverseRecord> {
    let path = root.join("files").join(file_record_name(rel));
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn file_record_name(rel: &str) -> String {
    let digest = Sha256::digest(rel.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{hex}.json")
}

struct BuiltReverseIndex {
    selectors: Vec<String>,
    files: BTreeMap<String, Vec<(u32, u32, Vec<u32>)>>,
}

type RawFileCoverage = BTreeMap<String, BTreeMap<String, BTreeSet<u32>>>;

fn build_reverse_line_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<BuiltReverseIndex, RustLlvmCovError> {
    let (raw, selector_set) = collect_raw_coverage_by_file(cache_root, source_root, generation)?;
    let selectors: Vec<String> = selector_set.into_iter().collect();
    let files = compress_file_coverage(&raw, &selectors);
    Ok(BuiltReverseIndex { selectors, files })
}

fn collect_raw_coverage_by_file(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<(RawFileCoverage, BTreeSet<String>), RustLlvmCovError> {
    let mut raw = RawFileCoverage::new();
    let mut selector_set = BTreeSet::new();
    let entries_dir = cache_root.join("entries");
    if !entries_dir.is_dir() {
        return Ok((raw, selector_set));
    }
    for entry in fs::read_dir(&entries_dir).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(parsed) = load_passed_generation_entry(&path, generation) else {
            continue;
        };
        selector_set.insert(parsed.selector.clone());
        absorb_entry_coverage(source_root, &parsed, &mut raw);
    }
    Ok((raw, selector_set))
}

fn load_passed_generation_entry(path: &Path, generation: &str) -> Option<RustCovCacheEntry> {
    let bytes = fs::read(path).ok()?;
    let parsed: RustCovCacheEntry = serde_json::from_slice(&bytes).ok()?;
    (parsed.schema_version == CACHE_SCHEMA_VERSION
        && parsed.generation_fingerprint == generation
        && parsed.status == TestStatus::Passed
        && !parsed.coverage.files.is_empty())
    .then_some(parsed)
}

fn absorb_entry_coverage(
    source_root: &Path,
    parsed: &RustCovCacheEntry,
    raw: &mut RawFileCoverage,
) {
    for (file, lines) in &parsed.coverage.files {
        let Some(rel) = repo_relative_coverage_file(source_root, file) else {
            continue;
        };
        raw.entry(rel)
            .or_default()
            .entry(parsed.selector.clone())
            .or_default()
            .extend(lines.iter().copied());
    }
}

fn compress_file_coverage(
    raw: &RawFileCoverage,
    selectors: &[String],
) -> BTreeMap<String, Vec<(u32, u32, Vec<u32>)>> {
    let selector_ids: BTreeMap<&str, u32> = selectors
        .iter()
        .enumerate()
        .map(|(idx, name)| (name.as_str(), u32::try_from(idx).unwrap_or(u32::MAX)))
        .collect();
    let mut files = BTreeMap::new();
    for (rel, by_selector) in raw {
        files.insert(rel.clone(), compress_selector_ranges(by_selector, &selector_ids));
    }
    files
}

fn compress_selector_ranges(
    by_selector: &BTreeMap<String, BTreeSet<u32>>,
    selector_ids: &BTreeMap<&str, u32>,
) -> Vec<(u32, u32, Vec<u32>)> {
    let mut range_map: BTreeMap<(u32, u32), BTreeSet<u32>> = BTreeMap::new();
    for (selector, lines) in by_selector {
        let Some(&sid) = selector_ids.get(selector.as_str()) else {
            continue;
        };
        for (start, end) in lines_to_ranges(lines) {
            range_map.entry((start, end)).or_default().insert(sid);
        }
    }
    range_map
        .into_iter()
        .map(|((start, end), ids)| (start, end, ids.into_iter().collect()))
        .collect()
}

fn lines_to_ranges(lines: &BTreeSet<u32>) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    let mut iter = lines.iter().copied();
    let Some(mut start) = iter.next() else {
        return ranges;
    };
    let mut end = start;
    for line in iter {
        if line == end + 1 {
            end = line;
            continue;
        }
        ranges.push((start, end));
        start = line;
        end = line;
    }
    ranges.push((start, end));
    ranges
}

fn write_reverse_line_index(
    cache_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
    built: &BuiltReverseIndex,
) -> Result<(), RustLlvmCovError> {
    let final_dir = reverse_line_index_dir(cache_root);
    let parent = cache_root;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp_dir = parent.join(format!(
        "reverse_line_index.next.{}",
        rust_cov_unique_suffix()
    ));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir).map_err(RustLlvmCovError::Io)?;
    }
    fs::create_dir_all(tmp_dir.join("files")).map_err(RustLlvmCovError::Io)?;
    write_json_atomic(
        &tmp_dir.join("selectors.json"),
        &built.selectors,
        "rust_reverse_selectors",
    )?;
    for (rel, ranges) in &built.files {
        let record = FileReverseRecord {
            file: rel.clone(),
            ranges: ranges.clone(),
        };
        write_json_atomic(
            &tmp_dir.join("files").join(file_record_name(rel)),
            &record,
            "rust_reverse_file",
        )?;
    }
    let meta = ReverseMeta {
        schema_version: REVERSE_LINE_INDEX_SCHEMA.to_string(),
        generation_fingerprint: generation.to_string(),
        entries_fingerprint: entries_fingerprint.to_string(),
    };
    write_json_atomic(&tmp_dir.join("meta.json"), &meta, "rust_reverse_meta")?;
    let backup = parent.join(format!(
        "reverse_line_index.prev.{}",
        rust_cov_unique_suffix()
    ));
    if final_dir.exists() {
        fs::rename(&final_dir, &backup).map_err(RustLlvmCovError::Io)?;
    }
    fs::rename(&tmp_dir, &final_dir).map_err(RustLlvmCovError::Io)?;
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn write_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
    barrier: &str,
) -> Result<(), RustLlvmCovError> {
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("reverse path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rev"),
        rust_cov_unique_suffix()
    ));
    let mut file = create_new_cache_file(&tmp).map_err(RustLlvmCovError::Io)?;
    serde_json::to_writer(&mut file, value).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to write reverse json: {err}"))
    })?;
    file.write_all(b"\n").map_err(RustLlvmCovError::Io)?;
    file.sync_all().map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_sync_before_rename(barrier, &tmp, path)
        .map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(&tmp, path).map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_rename(barrier, &tmp, path).map_err(RustLlvmCovError::Io)
}

#[cfg(test)]
#[path = "batch_reverse_line_index_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_reverse_line_index_rebuild_test.rs"]
mod rebuild_tests;
