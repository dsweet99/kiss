use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::publish_derived::batch_derived_index::read_population_generation;
use crate::rust_llvm_cov_runner::publish_derived::batch_io_skip_not_found::{
    dir_entry_ok_missing, dir_entry_path_ok_missing, file_type_ok_missing, read_dir_ok_missing,
    read_ok_missing, remove_dir_all_ok_missing, remove_file_ok_missing,
};
use crate::rust_llvm_cov_runner::rust_cov_cache::RustCovCacheEntry;
use crate::rust_llvm_cov_runner::{CACHE_SCHEMA_VERSION, RustLlvmCovError};
use std::collections::BTreeSet;
use std::path::Path;

const MAX_GENERATION_DIRS_PRUNED_PER_CALL: usize = 32;

pub(crate) fn prune_non_current_generations(
    cache_root: &Path,
    current_generation: &str,
) -> Result<usize, RustLlvmCovError> {
    prune_generations_except(
        cache_root,
        current_generation,
        retained_population_generation(cache_root),
    )
}

pub fn prune_obsolete_selective_generations(
    cache_root: &Path,
    current_generation: &str,
) -> Result<usize, RustLlvmCovError> {
    prune_generations_except(
        cache_root,
        current_generation,
        retained_population_generation(cache_root),
    )
}

pub(crate) fn maybe_prune_obsolete_selective_after_batch(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    result: &mut crate::rust_llvm_cov_runner::RustCoverageBatchResult,
) -> Result<(), RustLlvmCovError> {
    if result.batch_error.is_some() || req.population_publication_selectors.is_some() {
        return Ok(());
    }
    let pruned =
        prune_obsolete_selective_generations(&req.cache_root, &identity.generation_fingerprint)?;
    result.counters.cache_pruned_entries += pruned;
    Ok(())
}

fn retained_population_generation(cache_root: &Path) -> Option<String> {
    read_population_generation(cache_root)
}

fn prune_generations_except(
    cache_root: &Path,
    current_generation: &str,
    population_generation: Option<String>,
) -> Result<usize, RustLlvmCovError> {
    let retained = retained_generations(cache_root, current_generation, population_generation);
    let mut pruned = 0usize;
    if let Some(entries) =
        read_dir_ok_missing(&cache_root.join("entries")).map_err(RustLlvmCovError::Io)?
    {
        for entry in entries {
            if try_prune_entry(entry, &retained)? {
                pruned += 1;
            }
        }
    }
    pruned += prune_unreferenced_generation_dirs(cache_root, &retained)?;
    Ok(pruned)
}

fn retained_generations(
    cache_root: &Path,
    current_generation: &str,
    population_generation: Option<String>,
) -> BTreeSet<String> {
    let mut retained = BTreeSet::from([current_generation.to_string()]);
    if let Some(previous) =
        population_generation.filter(|generation| generation != current_generation)
    {
        retained.insert(previous);
    }
    extend_retained_with_pointer_and_pins(cache_root, &mut retained);
    retained
}

fn extend_retained_with_pointer_and_pins(cache_root: &Path, retained: &mut BTreeSet<String>) {
    if let Ok(Some(bytes)) = read_ok_missing(&cache_root.join("current_generation.json"))
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        for key in ["generation_id", "parent_generation_id"] {
            if let Some(id) = value
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                retained.insert(id.to_string());
            }
        }
    }
    for kind in ["pending_pins", "reader_pins"] {
        let Ok(Some(entries)) = read_dir_ok_missing(&cache_root.join(kind)) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(bytes) = std::fs::read(entry.path())
                && let Ok(text) = std::str::from_utf8(&bytes)
            {
                let id = text.trim();
                if !id.is_empty() {
                    retained.insert(id.to_string());
                }
            }
        }
    }
}

fn prune_unreferenced_generation_dirs(
    cache_root: &Path,
    retained: &BTreeSet<String>,
) -> Result<usize, RustLlvmCovError> {
    let Some(entries) =
        read_dir_ok_missing(&cache_root.join("generations")).map_err(RustLlvmCovError::Io)?
    else {
        return Ok(0);
    };
    let mut pruned = 0usize;
    for entry in entries {
        if pruned >= MAX_GENERATION_DIRS_PRUNED_PER_CALL {
            break;
        }
        let Some(dir_entry) = dir_entry_ok_missing(entry).map_err(RustLlvmCovError::Io)? else {
            continue;
        };
        let name = match dir_entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };
        if name.starts_with('.') || retained.contains(&name) {
            continue;
        }
        let Some(file_type) = file_type_ok_missing(&dir_entry).map_err(RustLlvmCovError::Io)?
        else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        remove_dir_all_ok_missing(&dir_entry.path()).map_err(RustLlvmCovError::Io)?;
        pruned += 1;
    }
    Ok(pruned)
}

fn try_prune_entry(
    entry: std::io::Result<std::fs::DirEntry>,
    retained: &BTreeSet<String>,
) -> Result<bool, RustLlvmCovError> {
    let Some(path) = dir_entry_path_ok_missing(entry).map_err(RustLlvmCovError::Io)? else {
        return Ok(false);
    };
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return Ok(false);
    }
    let Some(bytes) = read_ok_missing(&path).map_err(RustLlvmCovError::Io)? else {
        return Ok(false);
    };
    if !entry_is_obsolete_to_prune(&bytes, retained) {
        return Ok(false);
    }
    remove_file_ok_missing(&path).map_err(RustLlvmCovError::Io)?;
    Ok(true)
}

fn entry_is_obsolete_to_prune(bytes: &[u8], retained: &BTreeSet<String>) -> bool {
    let Ok(parsed): Result<RustCovCacheEntry, _> = serde_json::from_slice(bytes) else {
        return false;
    };
    parsed.schema_version == CACHE_SCHEMA_VERSION
        && !parsed.generation_fingerprint.is_empty()
        && !retained.contains(&parsed.generation_fingerprint)
}
