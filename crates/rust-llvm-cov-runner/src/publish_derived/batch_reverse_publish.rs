//! Immutable reverse-line-index snapshot publication and pruning.

use crate::publish_derived::batch_io_skip_not_found::{
    dir_entry_path_ok_missing, read_dir_ok_missing, remove_dir_all_ok_missing,
};
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
    let staged = prepare_staged_snapshot_dir(&snaps)?;
    let selectors_digest = write_selectors_json(&staged, built)?;
    let files_meta = write_file_records(&staged, built)?;
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
    activate_staged_snapshot(&snaps, &staged, &snapshot_id)?;
    Ok(ReversePublishInfo {
        schema_version: REVERSE_LINE_INDEX_SCHEMA.to_string(),
        snapshot_id,
        meta_digest: hex_digest(&meta_bytes),
        entry_state_revision,
    })
}

fn prepare_staged_snapshot_dir(snaps: &Path) -> Result<PathBuf, RustLlvmCovError> {
    let staged = snaps.join(format!(".staging.{}", rust_cov_unique_suffix()));
    if staged.exists() {
        remove_dir_all_ok_missing(&staged).map_err(|err| {
            io_msg(
                err,
                &format!("remove staged reverse snapshot {}", staged.display()),
            )
        })?;
    }
    fs::create_dir_all(staged.join("files")).map_err(|err| {
        io_msg(
            err,
            &format!("create staged reverse snapshot {}", staged.display()),
        )
    })?;
    Ok(staged)
}

fn write_selectors_json(
    staged: &Path,
    built: &BuiltReverseIndex,
) -> Result<String, RustLlvmCovError> {
    let selectors_bytes = write_json_bytes(
        &staged.join("selectors.json"),
        &built.selectors,
        "rust_reverse_selectors",
    )?;
    Ok(hex_digest(&selectors_bytes))
}

fn write_file_records(
    staged: &Path,
    built: &BuiltReverseIndex,
) -> Result<BTreeMap<String, FileMeta>, RustLlvmCovError> {
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
    Ok(files_meta)
}

fn activate_staged_snapshot(
    snaps: &Path,
    staged: &Path,
    snapshot_id: &str,
) -> Result<(), RustLlvmCovError> {
    let final_dir = snaps.join(snapshot_id);
    fs::rename(staged, &final_dir).map_err(|err| {
        io_msg(
            err,
            &format!(
                "reverse snapshot rename {} -> {}",
                staged.display(),
                final_dir.display()
            ),
        )
    })?;
    sync_dir(snaps)
}

fn io_msg(err: std::io::Error, context: &str) -> RustLlvmCovError {
    RustLlvmCovError::Io(std::io::Error::new(
        err.kind(),
        format!("{context}: {err}"),
    ))
}

pub fn prune_unreferenced_snapshots(
    cache_root: &Path,
    active_id: &str,
    prior_id: Option<&str>,
) -> Result<usize, RustLlvmCovError> {
    let snaps = snapshots_dir(cache_root);
    let Some(entries) = read_dir_ok_missing(&snaps).map_err(RustLlvmCovError::Io)? else {
        return Ok(0);
    };
    let mut removed = 0;
    for entry in entries {
        let Some(path) = dir_entry_path_ok_missing(entry).map_err(RustLlvmCovError::Io)? else {
            continue;
        };
        removed += prune_one_unreferenced_snapshot(&path, active_id, prior_id);
    }
    Ok(removed)
}

fn prune_one_unreferenced_snapshot(path: &Path, active_id: &str, prior_id: Option<&str>) -> usize {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return 0;
    };
    // Staging dirs (`.` prefix): another publisher may still own them; never fail.
    let keep = !name.starts_with('.') && (name == active_id || prior_id == Some(name));
    if keep {
        return 0;
    }
    usize::from(remove_dir_all_ok_missing(path).is_ok())
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
    let dir = File::open(path).map_err(|err| io_msg(err, &format!("sync_dir {}", path.display())))?;
    dir.sync_all().map_err(RustLlvmCovError::Io)
}
