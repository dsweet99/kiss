use crate::publish_derived::batch_derived_index::read_population_generation;
use crate::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::RustCovCacheEntry;
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

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

/// Selective runs prune obsolete result generations only after a successful batch.
/// Failed or interrupted runs must retain the complete population snapshot generation.
pub(crate) fn maybe_prune_obsolete_selective_after_batch(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    result: &mut crate::RustCoverageBatchResult,
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
    let entries_dir = cache_root.join("entries");
    if !entries_dir.is_dir() {
        return Ok(0);
    }
    let mut retained = BTreeSet::from([current_generation.to_string()]);
    if let Some(previous) =
        population_generation.filter(|generation| generation != current_generation)
    {
        retained.insert(previous);
    }
    let mut pruned = 0usize;
    for entry in fs::read_dir(&entries_dir).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(RustLlvmCovError::Io)?;
        let Ok(parsed): Result<RustCovCacheEntry, _> = serde_json::from_slice(&bytes) else {
            continue;
        };
        if parsed.schema_version != CACHE_SCHEMA_VERSION {
            continue;
        }
        if parsed.generation_fingerprint.is_empty()
            || retained.contains(&parsed.generation_fingerprint)
        {
            continue;
        }
        fs::remove_file(path).map_err(RustLlvmCovError::Io)?;
        pruned += 1;
    }
    Ok(pruned)
}
