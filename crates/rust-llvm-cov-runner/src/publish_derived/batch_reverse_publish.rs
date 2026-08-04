//! Immutable reverse-line-index snapshot publication and pruning.

use crate::publish_derived::batch_reverse_build::{
    BuiltReverseIndex, FileMeta, FileReverseRecord, ReverseMeta, ReversePublishInfo,
    REVERSE_LINE_INDEX_SCHEMA, build_reverse_line_index, file_record_name, hex_digest,
};
use crate::rust_cov_cache::rust_cov_unique_suffix;
use crate::RustLlvmCovError;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn reverse_line_index_dir(cache_root: &Path) -> PathBuf {
    cache_root.join("reverse_line_index")
}

pub fn snapshots_dir(cache_root: &Path) -> PathBuf {
    reverse_line_index_dir(cache_root).join("snapshots")
}

pub fn snapshot_path(cache_root: &Path, snapshot_id: &str) -> PathBuf {
    snapshots_dir(cache_root).join(snapshot_id)
}

pub fn publish_reverse_line_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
    entry_state_revision: u64,
) -> Result<ReversePublishInfo, RustLlvmCovError> {
    let built = build_reverse_line_index(cache_root, source_root, generation)?;
    write_reverse_snapshot(
        cache_root,
        generation,
        entries_fingerprint,
        entry_state_revision,
        &built,
    )
}

pub fn write_reverse_snapshot(
    cache_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
    entry_state_revision: u64,
    built: &BuiltReverseIndex,
) -> Result<ReversePublishInfo, RustLlvmCovError> {
    let snaps = snapshots_dir(cache_root);
    fs::create_dir_all(&snaps).map_err(RustLlvmCovError::Io)?;
    let snapshot_id = format!(
        "{}-{}-{}",
        short_id(generation),
        short_id(entries_fingerprint),
        rust_cov_unique_suffix()
    );
    let staged = snaps.join(format!(".staging.{}", rust_cov_unique_suffix()));
    if staged.exists() {
        fs::remove_dir_all(&staged).map_err(RustLlvmCovError::Io)?;
    }
    fs::create_dir_all(staged.join("files")).map_err(RustLlvmCovError::Io)?;
    let selectors_bytes = write_json_bytes(
        &staged.join("selectors.json"),
        &built.selectors,
        "rust_reverse_selectors",
    )?;
    let selectors_digest = hex_digest(&selectors_bytes);
    let mut files_meta = BTreeMap::new();
    for (rel, ranges) in &built.files {
        let record = FileReverseRecord {
            file: rel.clone(),
            ranges: ranges.clone(),
        };
        let name = file_record_name(rel);
        let bytes = write_json_bytes(
            &staged.join("files").join(&name),
            &record,
            "rust_reverse_file",
        )?;
        files_meta.insert(
            rel.clone(),
            FileMeta {
                record: name,
                digest: hex_digest(&bytes),
            },
        );
    }
    let meta = ReverseMeta {
        schema_version: REVERSE_LINE_INDEX_SCHEMA.to_string(),
        snapshot_id: snapshot_id.clone(),
        generation_fingerprint: generation.to_string(),
        entry_state_revision,
        entries_fingerprint: entries_fingerprint.to_string(),
        selectors_digest,
        files: files_meta,
    };
    let meta_bytes = write_json_bytes(&staged.join("meta.json"), &meta, "rust_reverse_meta")?;
    sync_dir(&staged)?;
    let final_dir = snaps.join(&snapshot_id);
    fs::rename(&staged, &final_dir).map_err(RustLlvmCovError::Io)?;
    sync_dir(&snaps)?;
    Ok(ReversePublishInfo {
        schema_version: REVERSE_LINE_INDEX_SCHEMA.to_string(),
        snapshot_id,
        meta_digest: hex_digest(&meta_bytes),
        entry_state_revision,
    })
}

pub fn prune_unreferenced_snapshots(
    cache_root: &Path,
    active_id: &str,
    prior_id: Option<&str>,
) -> Result<usize, RustLlvmCovError> {
    let snaps = snapshots_dir(cache_root);
    if !snaps.is_dir() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(&snaps).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            let _ = fs::remove_dir_all(&path);
            removed += 1;
            continue;
        }
        if name == active_id || prior_id == Some(name) {
            continue;
        }
        if fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn read_prior_snapshot_id(cache_root: &Path) -> Option<String> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("reverse_line_index")?
        .get("snapshot_id")?
        .as_str()
        .map(str::to_string)
}

fn short_id(value: &str) -> String {
    let digest = hex_digest(value.as_bytes());
    digest.chars().take(16).collect()
}

fn write_json_bytes<T: Serialize>(
    path: &Path,
    value: &T,
    barrier: &str,
) -> Result<Vec<u8>, RustLlvmCovError> {
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("reverse path has no parent".into()))?;
    let bytes = serde_json::to_vec(value).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to serialize reverse json: {err}"))
    })?;
    let mut with_nl = bytes.clone();
    with_nl.push(b'\n');
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rev"),
        rust_cov_unique_suffix()
    ));
    let payload = with_nl.clone();
    kiss_publication_barrier::publish_atomically(barrier, path, &tmp, |file| {
        file.write_all(&payload)
    })
    .map_err(RustLlvmCovError::Io)?;
    Ok(with_nl)
}

fn sync_dir(path: &Path) -> Result<(), RustLlvmCovError> {
    let dir = File::open(path).map_err(RustLlvmCovError::Io)?;
    dir.sync_all().map_err(RustLlvmCovError::Io)
}
