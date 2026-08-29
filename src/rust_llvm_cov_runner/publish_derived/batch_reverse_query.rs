use crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::{
    EntryState, read_entry_state,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_build::{
    FileReverseRecord, REVERSE_LINE_INDEX_SCHEMA, ReverseMeta, file_record_name, hex_digest,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_publish::snapshot_path;
use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_query_metrics::{
    ReverseUnavailableReason, record_reverse_hit, record_reverse_unavailable,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_query_metrics::{
    REVERSE_QUERY_HITS, ReverseQueryCounters, ReverseUnavailableCounts,
    snapshot_reverse_query_counters, take_reverse_query_counters_since_last_copy,
};

#[cfg(test)]
pub use crate::rust_llvm_cov_runner::publish_derived::batch_reverse_query_metrics::reset_reverse_query_counters_for_test;

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
        Ok(out) => {
            record_reverse_hit();
            Some(out)
        }
        Err(reason) => {
            record_reverse_unavailable(reason);
            None
        }
    }
}

fn query_validated(
    cache_root: &Path,
    generation: &str,
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Result<BTreeMap<String, BTreeSet<String>>, ReverseUnavailableReason> {
    let needle = load_population_needle(cache_root)?;
    let reverse = needle
        .reverse_line_index
        .as_ref()
        .ok_or(ReverseUnavailableReason::MissingRecord)?;
    if reverse.schema_version != REVERSE_LINE_INDEX_SCHEMA {
        return Err(ReverseUnavailableReason::Schema);
    }
    if needle.generation_fingerprint != generation {
        return Err(ReverseUnavailableReason::Generation);
    }
    let state = read_entry_state(cache_root).ok_or(ReverseUnavailableReason::MissingRecord)?;
    classify_entry_state(&state, &needle, reverse)?;
    let root = snapshot_path(cache_root, &reverse.snapshot_id);
    let meta_bytes =
        fs::read(root.join("meta.json")).map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    if hex_digest(&meta_bytes) != reverse.meta_digest {
        return Err(ReverseUnavailableReason::Digest);
    }
    let meta: ReverseMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| ReverseUnavailableReason::Malformed)?;
    classify_meta(&meta, &needle, reverse)?;
    let selectors_bytes = fs::read(root.join("selectors.json"))
        .map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    if hex_digest(&selectors_bytes) != meta.selectors_digest {
        return Err(ReverseUnavailableReason::Digest);
    }
    let selectors: Vec<String> = serde_json::from_slice(&selectors_bytes)
        .map_err(|_| ReverseUnavailableReason::Malformed)?;
    select_from_snapshot(&root, &meta, &selectors, changed_rels)
}

fn classify_entry_state(
    state: &EntryState,
    needle: &PopulationReverseNeedle,
    reverse: &ReverseNeedle,
) -> Result<(), ReverseUnavailableReason> {
    if state.generation_fingerprint != needle.generation_fingerprint {
        return Err(ReverseUnavailableReason::Generation);
    }
    if state.entries_fingerprint != needle.entries_fingerprint {
        return Err(ReverseUnavailableReason::Fingerprint);
    }
    if state.revision != reverse.entry_state_revision {
        return Err(ReverseUnavailableReason::Revision);
    }
    Ok(())
}

fn classify_meta(
    meta: &ReverseMeta,
    needle: &PopulationReverseNeedle,
    reverse: &ReverseNeedle,
) -> Result<(), ReverseUnavailableReason> {
    if meta.schema_version != REVERSE_LINE_INDEX_SCHEMA {
        return Err(ReverseUnavailableReason::Schema);
    }
    if meta.snapshot_id != reverse.snapshot_id {
        return Err(ReverseUnavailableReason::MissingRecord);
    }
    if meta.generation_fingerprint != needle.generation_fingerprint {
        return Err(ReverseUnavailableReason::Generation);
    }
    if meta.entries_fingerprint != needle.entries_fingerprint {
        return Err(ReverseUnavailableReason::Fingerprint);
    }
    if meta.entry_state_revision != reverse.entry_state_revision {
        return Err(ReverseUnavailableReason::Revision);
    }
    Ok(())
}

fn load_population_needle(
    cache_root: &Path,
) -> Result<PopulationReverseNeedle, ReverseUnavailableReason> {
    let bytes = fs::read(cache_root.join("population.json"))
        .map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    serde_json::from_slice(&bytes).map_err(|_| ReverseUnavailableReason::Malformed)
}

fn select_from_snapshot(
    root: &Path,
    meta: &ReverseMeta,
    selectors: &[String],
    changed_rels: &BTreeMap<String, BTreeSet<u32>>,
) -> Result<BTreeMap<String, BTreeSet<String>>, ReverseUnavailableReason> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (rel, wanted) in changed_rels {
        if wanted.is_empty() {
            continue;
        }
        match meta.files.get(rel) {
            None => {}
            Some(file_meta) => {
                let record = load_validated_record(root, rel, file_meta)?;
                let selected = selectors_for_wanted(&record, selectors, wanted)?;
                if !selected.is_empty() {
                    out.insert(rel.clone(), selected);
                }
            }
        }
    }
    Ok(out)
}

fn load_validated_record(
    root: &Path,
    rel: &str,
    file_meta: &crate::rust_llvm_cov_runner::publish_derived::batch_reverse_build::FileMeta,
) -> Result<FileReverseRecord, ReverseUnavailableReason> {
    if file_meta.record != file_record_name(rel) {
        return Err(ReverseUnavailableReason::MissingRecord);
    }
    let path = root.join("files").join(&file_meta.record);
    let bytes = fs::read(path).map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    if hex_digest(&bytes) != file_meta.digest {
        return Err(ReverseUnavailableReason::Digest);
    }
    let record: FileReverseRecord =
        serde_json::from_slice(&bytes).map_err(|_| ReverseUnavailableReason::Malformed)?;
    if record.file != *rel {
        return Err(ReverseUnavailableReason::Malformed);
    }
    Ok(record)
}

fn selectors_for_wanted(
    record: &FileReverseRecord,
    selectors: &[String],
    wanted: &BTreeSet<u32>,
) -> Result<BTreeSet<String>, ReverseUnavailableReason> {
    let mut selected = BTreeSet::new();
    for (start, end, ids) in &record.ranges {
        if !range_overlaps_wanted(*start, *end, wanted) {
            continue;
        }
        for id in ids {
            let sel = selectors
                .get(*id as usize)
                .ok_or(ReverseUnavailableReason::Malformed)?;
            selected.insert(sel.clone());
        }
    }
    Ok(selected)
}

pub fn query_reverse_covering_files(
    cache_root: &Path,
    generation: &str,
    rels: &[String],
) -> Option<BTreeSet<String>> {
    if rels.is_empty() {
        return Some(BTreeSet::new());
    }
    match covering_files_validated(cache_root, generation, rels) {
        Ok(out) => {
            record_reverse_hit();
            Some(out)
        }
        Err(reason) => {
            record_reverse_unavailable(reason);
            None
        }
    }
}

fn covering_files_validated(
    cache_root: &Path,
    generation: &str,
    rels: &[String],
) -> Result<BTreeSet<String>, ReverseUnavailableReason> {
    let needle = load_population_needle(cache_root)?;
    let reverse = needle
        .reverse_line_index
        .as_ref()
        .ok_or(ReverseUnavailableReason::MissingRecord)?;
    if reverse.schema_version != REVERSE_LINE_INDEX_SCHEMA {
        return Err(ReverseUnavailableReason::Schema);
    }
    if needle.generation_fingerprint != generation {
        return Err(ReverseUnavailableReason::Generation);
    }
    let state = read_entry_state(cache_root).ok_or(ReverseUnavailableReason::MissingRecord)?;
    classify_entry_state(&state, &needle, reverse)?;
    let root = snapshot_path(cache_root, &reverse.snapshot_id);
    let meta_bytes =
        fs::read(root.join("meta.json")).map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    if hex_digest(&meta_bytes) != reverse.meta_digest {
        return Err(ReverseUnavailableReason::Digest);
    }
    let meta: ReverseMeta =
        serde_json::from_slice(&meta_bytes).map_err(|_| ReverseUnavailableReason::Malformed)?;
    classify_meta(&meta, &needle, reverse)?;
    let selectors_bytes = fs::read(root.join("selectors.json"))
        .map_err(|_| ReverseUnavailableReason::MissingRecord)?;
    if hex_digest(&selectors_bytes) != meta.selectors_digest {
        return Err(ReverseUnavailableReason::Digest);
    }
    let selectors: Vec<String> = serde_json::from_slice(&selectors_bytes)
        .map_err(|_| ReverseUnavailableReason::Malformed)?;
    let mut out = BTreeSet::new();
    for rel in rels {
        match meta.files.get(rel) {
            None => return Err(ReverseUnavailableReason::MissingRecord),
            Some(file_meta) => {
                let record = load_validated_record(&root, rel, file_meta)?;
                for (_start, _end, ids) in &record.ranges {
                    for id in ids {
                        let sel = selectors
                            .get(*id as usize)
                            .ok_or(ReverseUnavailableReason::Malformed)?;
                        out.insert(sel.clone());
                    }
                }
            }
        }
    }
    Ok(out)
}

fn range_overlaps_wanted(start: u32, end: u32, wanted: &BTreeSet<u32>) -> bool {
    wanted.range(start..=end).next().is_some()
}
