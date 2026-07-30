//! Reverse-bound population validation without rescanning entries/*.json.

use crate::batch_derived_index_types::{
    OnDiskIndexWithFiles, PopulationManifestOnDisk, ReverseLineIndexManifestMeta,
};
use crate::batch_reverse_build::{ReverseMeta, hex_digest};
use crate::batch_reverse_publish::snapshot_path;
use std::fs;
use std::path::Path;

pub(crate) fn reverse_bound_index_ok(
    cache_root: &Path,
    manifest: &PopulationManifestOnDisk,
    index: &OnDiskIndexWithFiles,
) -> bool {
    let Some(reverse) = manifest.reverse_line_index.as_ref() else {
        return false;
    };
    let Some(state) = crate::batch_entry_state::read_entry_state(cache_root) else {
        return false;
    };
    crate::batch_entry_state::entry_state_matches(
        &state,
        &manifest.generation_fingerprint,
        &manifest.entries_fingerprint,
        reverse.entry_state_revision,
    ) && index_keys_match_reverse_snapshot(cache_root, reverse, index)
}

fn index_keys_match_reverse_snapshot(
    cache_root: &Path,
    reverse: &ReverseLineIndexManifestMeta,
    index: &OnDiskIndexWithFiles,
) -> bool {
    let meta_path = snapshot_path(cache_root, &reverse.snapshot_id).join("meta.json");
    let Ok(meta_bytes) = fs::read(&meta_path) else {
        return false;
    };
    if hex_digest(&meta_bytes) != reverse.meta_digest {
        return false;
    }
    let Ok(meta) = serde_json::from_slice::<ReverseMeta>(&meta_bytes) else {
        return false;
    };
    index.files.keys().eq(meta.files.keys())
}
