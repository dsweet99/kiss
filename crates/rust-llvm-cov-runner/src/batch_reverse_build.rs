//! Build reverse line coverage from authoritative selector entries.

use crate::rust_cov_cache::{RustCovCacheEntry, repo_relative_coverage_file};
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError};
use rpytest_runner::TestStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const REVERSE_LINE_INDEX_SCHEMA: &str = "rust-llvm-cov-reverse-v2";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReverseMeta {
    pub schema_version: String,
    pub snapshot_id: String,
    pub generation_fingerprint: String,
    pub entry_state_revision: u64,
    pub entries_fingerprint: String,
    pub selectors_digest: String,
    pub files: BTreeMap<String, FileMeta>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMeta {
    pub record: String,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileReverseRecord {
    pub file: String,
    pub ranges: Vec<(u32, u32, Vec<u32>)>,
}

#[derive(Clone, Debug)]
pub struct BuiltReverseIndex {
    pub selectors: Vec<String>,
    pub files: BTreeMap<String, Vec<(u32, u32, Vec<u32>)>>,
}

#[derive(Clone, Debug)]
pub struct ReversePublishInfo {
    pub schema_version: String,
    pub snapshot_id: String,
    pub meta_digest: String,
    pub entry_state_revision: u64,
}

type RawFileCoverage = BTreeMap<String, BTreeMap<String, BTreeSet<u32>>>;

pub fn build_reverse_line_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<BuiltReverseIndex, RustLlvmCovError> {
    let (raw, selector_set) = collect_raw_coverage_by_file(cache_root, source_root, generation)?;
    let selectors: Vec<String> = selector_set.into_iter().collect();
    let files = compress_file_coverage(&raw, &selectors);
    Ok(BuiltReverseIndex { selectors, files })
}

pub fn file_record_name(rel: &str) -> String {
    format!("{}.json", hex_digest(rel.as_bytes()))
}

pub fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
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
