//! Strict manifest-bound reverse snapshot reads with safe fallback.

use crate::batch_entry_state::{entry_state_matches, read_entry_state};
use crate::batch_reverse_build::{
    FileReverseRecord, ReverseMeta, REVERSE_LINE_INDEX_SCHEMA, file_record_name, hex_digest,
};
use crate::batch_reverse_publish::snapshot_path;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub static REVERSE_QUERY_HITS: AtomicU64 = AtomicU64::new(0);
pub static REVERSE_QUERY_UNAVAILABLE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize)]
struct PopulationReverseNeedle {
    generation_fingerprint: String,
    entries_fingerprint: String,
    reverse_line_index: Option<ReverseNeedle>,
}

#[derive(Clone, Debug, Deserialize)]
struct ReverseNeedle {
    schema_version: String,
    snapshot_id: String,
    meta_digest: String,
    entry_state_revision: u64,
}

pub fn query_reverse_line_index(
    cache_root: &Path,
    generation: &str,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    if changed_rels.is_empty() {
        return Some(BTreeMap::new());
    }
    match query_validated(cache_root, generation, changed_rels) {
        Some(out) => {
            REVERSE_QUERY_HITS.fetch_add(1, Ordering::Relaxed);
            Some(out)
        }
        None => {
            REVERSE_QUERY_UNAVAILABLE.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

fn query_validated(
    cache_root: &Path,
    generation: &str,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let needle = load_population_needle(cache_root)?;
    let reverse = needle.reverse_line_index.as_ref()?;
    if needle.generation_fingerprint != generation
        || reverse.schema_version != REVERSE_LINE_INDEX_SCHEMA
    {
        return None;
    }
    let state = read_entry_state(cache_root)?;
    if !entry_state_matches(
        &state,
        &needle.generation_fingerprint,
        &needle.entries_fingerprint,
        reverse.entry_state_revision,
    ) {
        return None;
    }
    let root = snapshot_path(cache_root, &reverse.snapshot_id);
    let meta_bytes = fs::read(root.join("meta.json")).ok()?;
    if hex_digest(&meta_bytes) != reverse.meta_digest {
        return None;
    }
    let meta: ReverseMeta = serde_json::from_slice(&meta_bytes).ok()?;
    if !meta_matches_needle(&meta, &needle, reverse) {
        return None;
    }
    let selectors_bytes = fs::read(root.join("selectors.json")).ok()?;
    if hex_digest(&selectors_bytes) != meta.selectors_digest {
        return None;
    }
    let selectors: Vec<String> = serde_json::from_slice(&selectors_bytes).ok()?;
    select_from_snapshot(&root, &meta, &selectors, changed_rels)
}

fn load_population_needle(cache_root: &Path) -> Option<PopulationReverseNeedle> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn meta_matches_needle(
    meta: &ReverseMeta,
    needle: &PopulationReverseNeedle,
    reverse: &ReverseNeedle,
) -> bool {
    meta.schema_version == REVERSE_LINE_INDEX_SCHEMA
        && meta.snapshot_id == reverse.snapshot_id
        && meta.generation_fingerprint == needle.generation_fingerprint
        && meta.entries_fingerprint == needle.entries_fingerprint
        && meta.entry_state_revision == reverse.entry_state_revision
}

fn select_from_snapshot(
    root: &Path,
    meta: &ReverseMeta,
    selectors: &[String],
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (rel, wanted) in changed_rels {
        if wanted.is_empty() {
            continue;
        }
        match meta.files.get(rel) {
            None => {
                // Absent from complete map: trusted empty selector set.
            }
            Some(file_meta) => {
                let record = load_validated_record(root, rel, file_meta)?;
                let selected = selectors_for_wanted(&record, selectors, wanted)?;
                if !selected.is_empty() {
                    out.insert(rel.clone(), selected);
                }
            }
        }
    }
    Some(out)
}

fn load_validated_record(
    root: &Path,
    rel: &str,
    file_meta: &crate::batch_reverse_build::FileMeta,
) -> Option<FileReverseRecord> {
    if file_meta.record != file_record_name(rel) {
        return None;
    }
    let path = root.join("files").join(&file_meta.record);
    let bytes = fs::read(path).ok()?;
    if hex_digest(&bytes) != file_meta.digest {
        return None;
    }
    let record: FileReverseRecord = serde_json::from_slice(&bytes).ok()?;
    (record.file == *rel).then_some(record)
}

fn selectors_for_wanted(
    record: &FileReverseRecord,
    selectors: &[String],
    wanted: &BTreeSet<u32>,
) -> Option<BTreeSet<String>> {
    let mut selected = BTreeSet::new();
    for (start, end, ids) in &record.ranges {
        if !range_overlaps_wanted(*start, *end, wanted) {
            continue;
        }
        for id in ids {
            let sel = selectors.get(*id as usize)?;
            selected.insert(sel.clone());
        }
    }
    Some(selected)
}

fn range_overlaps_wanted(start: u32, end: u32, wanted: &BTreeSet<u32>) -> bool {
    wanted.range(start..=end).next().is_some()
}
