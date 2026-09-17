use std::fs;
use std::path::Path;

use super::lock::publication_lock;
use super::paths::generation_dir;
use super::pin::{PinGuard, acquire_reader_pin};
use super::pointer::read_pointer;
use super::types::{FullExecutionGeneration, GENERATION_SCHEMA_VERSION};

#[cfg(test)]
static LOAD_CURRENT_GENERATION_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static COUNTED_CACHE_ROOT: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

pub(crate) fn load_current_generation(
    cache_root: &Path,
) -> Result<(FullExecutionGeneration, PinGuard), String> {
    #[cfg(test)]
    if COUNTED_CACHE_ROOT
        .lock()
        .ok()
        .and_then(|root| root.clone())
        .as_deref()
        == Some(cache_root)
    {
        LOAD_CURRENT_GENERATION_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    let pin = {
        let _guard = publication_lock(cache_root)?;
        let pointer = read_pointer(cache_root)?
            .ok_or_else(|| "error: kiss: missing generation pointer".to_string())?;
        acquire_reader_pin(cache_root, &pointer.generation_id)?
    };
    let generation = read_pointed_generation(cache_root)?;
    Ok((generation, pin))
}

#[cfg(test)]
pub(crate) fn reset_load_current_generation_call_count(cache_root: &Path) {
    LOAD_CURRENT_GENERATION_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut root) = COUNTED_CACHE_ROOT.lock() {
        *root = Some(cache_root.to_path_buf());
    }
}

#[cfg(test)]
pub(crate) fn load_current_generation_call_count() -> usize {
    LOAD_CURRENT_GENERATION_CALLS.load(std::sync::atomic::Ordering::Relaxed)
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
