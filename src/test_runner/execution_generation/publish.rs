use std::fs;
use std::path::Path;

use super::digest::{generation_id_for_payload, manifest_digest};
use super::gc::remove_staging_dirs;
use super::lock::publication_lock;
use super::paths::{
    create_staging_dir, generation_dir, generations_dir, sync_dir, write_create_new_bytes,
};
use super::pointer::{read_pointer, write_pointer_cas};
use super::types::{CurrentGenerationPointer, FullExecutionGeneration, GENERATION_SCHEMA_VERSION};

pub(crate) fn publish_full_generation(
    cache_root: &Path,
    generation: FullExecutionGeneration,
) -> Result<String, String> {
    if !generation.is_complete_all_pass() {
        return Err("error: kiss: refusing incomplete Full generation publication".into());
    }
    let _guard = publication_lock(cache_root)?;
    remove_staging_dirs(cache_root);
    let parent = read_pointer(cache_root)?;
    let parent_id = parent
        .as_ref()
        .map(|pointer| pointer.generation_id.clone())
        .unwrap_or_default();
    match commit_generation_under_lock(cache_root, generation.clone(), &parent_id) {
        Ok(generation_id) => Ok(generation_id),
        Err(err) if err.contains("stale generation writer") => {
            rebase_stale_writer(cache_root, &generation)
        }
        Err(err) => Err(err),
    }
}

fn rebase_stale_writer(
    cache_root: &Path,
    incoming: &FullExecutionGeneration,
) -> Result<String, String> {
    let current = super::load::read_pointed_generation(cache_root)?;
    let merged = super::rebase::rebase_incoming_on_current(&current, incoming)?;
    commit_generation_under_lock(cache_root, merged, &current.generation_id)
}

fn commit_generation_under_lock(
    cache_root: &Path,
    mut generation: FullExecutionGeneration,
    parent_id: &str,
) -> Result<String, String> {
    generation.schema_version = GENERATION_SCHEMA_VERSION.to_string();
    generation.generation_id.clear();
    generation.content_digest.clear();
    let generation_id = generation_id_for_payload(&generation)?;
    generation.generation_id = generation_id.clone();
    generation.content_digest = manifest_digest(&generation)?;
    let _pending = super::pin::acquire_pending_pin(cache_root, &generation_id)?;
    let staged = create_staging_dir(cache_root)?;
    let mut bytes = serde_json::to_vec(&generation)
        .map_err(|err| format!("error: kiss: serialize generation: {err}"))?;
    bytes.push(b'\n');
    write_create_new_bytes(&staged.join("generation.json"), &bytes)?;
    write_evidence_blobs(&staged, &generation)?;
    sync_dir(&staged)?;
    let final_dir = generation_dir(cache_root, &generation_id);
    if final_dir.exists() {
        let _ = fs::remove_dir_all(&staged);
    } else {
        fs::rename(&staged, &final_dir)
            .map_err(|err| format!("error: kiss: rename generation staging: {err}"))?;
        sync_dir(&generations_dir(cache_root))?;
    }
    let pointer = CurrentGenerationPointer {
        schema_version: super::pointer::POINTER_SCHEMA_VERSION.to_string(),
        generation_id: generation_id.clone(),
        generation_manifest_digest: generation.content_digest.clone(),
        parent_generation_id: parent_id.to_string(),
    };
    let expected = if parent_id.is_empty() {
        None
    } else {
        Some(parent_id)
    };
    write_pointer_cas(cache_root, &pointer, expected)?;
    Ok(generation_id)
}

fn write_evidence_blobs(dir: &Path, generation: &FullExecutionGeneration) -> Result<(), String> {
    let evidence_dir = dir.join("evidence");
    fs::create_dir_all(&evidence_dir)
        .map_err(|err| format!("error: kiss: create evidence dir: {err}"))?;
    for record in &generation.selector_evidence {
        if record.entry_content_digest.is_empty() {
            return Err("error: kiss: missing entry_content_digest".into());
        }
        let path = evidence_dir.join(format!("{}.json", record.entry_content_digest));
        if path.exists() {
            continue;
        }
        let mut bytes = serde_json::to_vec(record)
            .map_err(|err| format!("error: kiss: serialize evidence blob: {err}"))?;
        bytes.push(b'\n');
        match write_create_new_bytes(&path, &bytes) {
            Ok(()) => {}
            Err(_) if path.exists() => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}
