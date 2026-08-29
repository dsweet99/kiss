use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paths::{pointer_file, sync_dir, write_create_new_bytes};
use super::types::CurrentGenerationPointer;
use kiss::kiss_publication_barrier::unique_process_suffix;

pub(crate) const POINTER_FILE_NAME: &str = "current_generation.json";
pub(crate) const POINTER_SCHEMA_VERSION: &str = "kiss-execution-generation-pointer-v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PointerFile {
    schema_version: String,
    generation_id: String,
    generation_manifest_digest: String,
    #[serde(default)]
    parent_generation_id: String,
}

pub(crate) fn read_pointer(cache_root: &Path) -> Result<Option<CurrentGenerationPointer>, String> {
    let path = pointer_file(cache_root, POINTER_FILE_NAME);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "error: kiss: read generation pointer {}: {err}",
                path.display()
            ));
        }
    };
    let parsed: PointerFile = serde_json::from_slice(&bytes)
        .map_err(|err| format!("error: kiss: parse generation pointer: {err}"))?;
    if parsed.schema_version != POINTER_SCHEMA_VERSION {
        return Err(format!(
            "error: kiss: unsupported generation pointer schema {}",
            parsed.schema_version
        ));
    }
    Ok(Some(CurrentGenerationPointer {
        schema_version: parsed.schema_version,
        generation_id: parsed.generation_id,
        generation_manifest_digest: parsed.generation_manifest_digest,
        parent_generation_id: parsed.parent_generation_id,
    }))
}

pub(crate) fn write_pointer_cas(
    cache_root: &Path,
    pointer: &CurrentGenerationPointer,
    expected_parent: Option<&str>,
) -> Result<(), String> {
    let current = read_pointer(cache_root)?;
    let current_id = current.as_ref().map(|item| item.generation_id.as_str());
    if current_id != expected_parent {
        return Err(format!(
            "error: kiss: stale generation writer parent={} current={}",
            expected_parent.unwrap_or(""),
            current_id.unwrap_or("")
        ));
    }
    let path = pointer_file(cache_root, POINTER_FILE_NAME);
    let tmp = path.with_file_name(format!(
        ".current_generation.{}.tmp",
        unique_process_suffix()
    ));
    let body = PointerFile {
        schema_version: POINTER_SCHEMA_VERSION.to_string(),
        generation_id: pointer.generation_id.clone(),
        generation_manifest_digest: pointer.generation_manifest_digest.clone(),
        parent_generation_id: pointer.parent_generation_id.clone(),
    };
    let mut bytes = serde_json::to_vec(&body)
        .map_err(|err| format!("error: kiss: serialize generation pointer: {err}"))?;
    bytes.push(b'\n');
    write_create_new_bytes(&tmp, &bytes)?;
    let still = read_pointer(cache_root)?;
    let still_id = still.as_ref().map(|item| item.generation_id.as_str());
    if still_id != expected_parent {
        let _ = fs::remove_file(&tmp);
        return Err("error: kiss: stale generation writer after pointer staging".into());
    }
    fs::rename(&tmp, &path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!("error: kiss: commit generation pointer: {err}")
    })?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}
