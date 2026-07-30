//! Lock-owned entry-state token for Rust coverage derived-state freshness.

use crate::rust_cov_cache::{create_new_cache_file, rust_cov_unique_suffix};
use crate::RustLlvmCovError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const ENTRY_STATE_SCHEMA: &str = "rust-llvm-cov-entry-state-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntryState {
    pub schema_version: String,
    pub generation_fingerprint: String,
    pub revision: u64,
    pub entries_fingerprint: String,
}

pub fn entry_state_path(cache_root: &Path) -> PathBuf {
    cache_root.join("entry_state.json")
}

pub fn read_entry_state(cache_root: &Path) -> Option<EntryState> {
    let bytes = fs::read(entry_state_path(cache_root)).ok()?;
    let state: EntryState = serde_json::from_slice(&bytes).ok()?;
    (state.schema_version == ENTRY_STATE_SCHEMA).then_some(state)
}

pub fn invalidate_entry_state(cache_root: &Path) {
    let _ = fs::remove_file(entry_state_path(cache_root));
}

pub fn publish_next_entry_state(
    cache_root: &Path,
    generation_fingerprint: &str,
    entries_fingerprint: &str,
) -> Result<u64, RustLlvmCovError> {
    let revision = read_entry_state(cache_root)
        .map(|state| state.revision.saturating_add(1))
        .unwrap_or(1);
    let state = EntryState {
        schema_version: ENTRY_STATE_SCHEMA.to_string(),
        generation_fingerprint: generation_fingerprint.to_string(),
        revision,
        entries_fingerprint: entries_fingerprint.to_string(),
    };
    write_entry_state(cache_root, &state)?;
    Ok(revision)
}

pub fn entry_state_matches(
    state: &EntryState,
    generation_fingerprint: &str,
    entries_fingerprint: &str,
    revision: u64,
) -> bool {
    state.schema_version == ENTRY_STATE_SCHEMA
        && state.generation_fingerprint == generation_fingerprint
        && state.entries_fingerprint == entries_fingerprint
        && state.revision == revision
}

fn write_entry_state(cache_root: &Path, state: &EntryState) -> Result<(), RustLlvmCovError> {
    let path = entry_state_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("entry_state has no parent".into()))?;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp = parent.join(format!(".entry_state.{}.tmp", rust_cov_unique_suffix()));
    let mut file = create_new_cache_file(&tmp).map_err(RustLlvmCovError::Io)?;
    serde_json::to_writer(&mut file, state).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to write entry_state: {err}"))
    })?;
    file.write_all(b"\n").map_err(RustLlvmCovError::Io)?;
    file.sync_all().map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_sync_before_rename("rust_entry_state", &tmp, &path)
        .map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(&tmp, &path).map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_rename("rust_entry_state", &tmp, &path)
        .map_err(RustLlvmCovError::Io)?;
    sync_dir(parent)
}

fn sync_dir(path: &Path) -> Result<(), RustLlvmCovError> {
    let dir = fs::File::open(path).map_err(RustLlvmCovError::Io)?;
    dir.sync_all().map_err(RustLlvmCovError::Io)
}

#[cfg(test)]
#[path = "batch_entry_state_test.rs"]
mod tests;
