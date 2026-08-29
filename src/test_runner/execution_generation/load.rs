use std::fs;
use std::path::Path;

use super::lock::publication_lock;
use super::paths::generation_dir;
use super::pin::{PinGuard, acquire_reader_pin};
use super::pointer::read_pointer;
use super::types::{FullExecutionGeneration, GENERATION_SCHEMA_VERSION};

pub(crate) fn load_current_generation(
    cache_root: &Path,
) -> Result<(FullExecutionGeneration, PinGuard), String> {
    let pin = {
        let _guard = publication_lock(cache_root)?;
        let pointer = read_pointer(cache_root)?
            .ok_or_else(|| "error: kiss: missing generation pointer".to_string())?;
        acquire_reader_pin(cache_root, &pointer.generation_id)?
    };
    let generation = read_pointed_generation(cache_root)?;
    Ok((generation, pin))
}

pub(super) fn read_pointed_generation(
    cache_root: &Path,
) -> Result<FullExecutionGeneration, String> {
    let pointer = read_pointer(cache_root)?
        .ok_or_else(|| "error: kiss: missing generation pointer".to_string())?;
    let path = generation_dir(cache_root, &pointer.generation_id).join("generation.json");
    let bytes = fs::read(&path)
        .map_err(|err| format!("error: kiss: read generation {}: {err}", path.display()))?;
    let generation: FullExecutionGeneration = serde_json::from_slice(&bytes)
        .map_err(|err| format!("error: kiss: parse generation: {err}"))?;
    if generation.schema_version != GENERATION_SCHEMA_VERSION {
        return Err(format!(
            "error: kiss: unsupported generation schema {}",
            generation.schema_version
        ));
    }
    let expected = super::digest::manifest_digest(&generation)?;
    if generation.content_digest != expected
        || generation.content_digest != pointer.generation_manifest_digest
        || generation.generation_id != pointer.generation_id
    {
        return Err("error: kiss: generation digest mismatch".into());
    }
    Ok(generation)
}
