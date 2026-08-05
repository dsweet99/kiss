//! Compact population wall-duration sidecar for warm `kiss cov` time gates.
//!
//! Authoritative durations still live in per-selector entry JSON. Warm loads of
//! thousands of full coverage entries dominate wall time, so derived state can
//! publish (and warm loads can rebuild) a small sidecar keyed to the current
//! population identity.

use crate::publish_derived::batch_derived_index::RustPopulationState;
use crate::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint,
};
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{load_rust_cov_cache_entry, rust_cov_unique_suffix};
use crate::CACHE_SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const POPULATION_DURATIONS_SCHEMA: &str = "rust-population-durations-v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PopulationDurationsFile {
    schema_version: String,
    cache_schema_version: String,
    generation_fingerprint: String,
    input_fingerprint: String,
    entries_fingerprint: String,
    /// selector -> duration as nanoseconds
    durations: BTreeMap<String, u64>,
}

pub(crate) fn population_durations_path(cache_root: &Path) -> PathBuf {
    cache_root.join("population_durations.json")
}

pub(crate) fn invalidate_population_durations(cache_root: &Path) {
    let _ = fs::remove_file(population_durations_path(cache_root));
}

/// Load validated wall durations for every selector in the current population.
///
/// Prefers `population_durations.json` when it matches the population identity.
/// On miss, loads per-selector entry durations and publishes the sidecar.
///
/// Returns `None` when the population or any selector entry is missing/invalid.
pub fn load_current_population_durations(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selectors: Option<&[String]>,
) -> Option<Vec<(String, Duration)>> {
    let population = crate::publish_derived::batch_derived_index::load_current_population_state(
        cache_root,
        source_root,
        identity,
        selectors,
    )?;
    if let Some(cached) = try_load_population_durations(cache_root, &population) {
        return Some(cached);
    }
    let out = load_durations_from_entries(cache_root, &population, identity, req, tools)?;
    let _ = write_population_durations(cache_root, &population, &out);
    Some(out)
}

pub(crate) fn try_load_population_durations(
    cache_root: &Path,
    population: &RustPopulationState,
) -> Option<Vec<(String, Duration)>> {
    let bytes = fs::read(population_durations_path(cache_root)).ok()?;
    let file: PopulationDurationsFile = serde_json::from_slice(&bytes).ok()?;
    if file.schema_version != POPULATION_DURATIONS_SCHEMA
        || file.cache_schema_version != CACHE_SCHEMA_VERSION
        || file.generation_fingerprint != population.generation_fingerprint
        || file.input_fingerprint != population.input_fingerprint
        || file.entries_fingerprint != population.entries_fingerprint
    {
        return None;
    }
    let mut out = Vec::with_capacity(population.selectors.len());
    for selector in &population.selectors {
        let nanos = *file.durations.get(selector)?;
        out.push((selector.clone(), Duration::from_nanos(nanos)));
    }
    Some(out)
}

pub(crate) fn write_population_durations(
    cache_root: &Path,
    population: &RustPopulationState,
    pairs: &[(String, Duration)],
) -> io::Result<()> {
    let mut durations = BTreeMap::new();
    for (selector, duration) in pairs {
        durations.insert(selector.clone(), duration_as_nanos(*duration));
    }
    if durations.len() != population.selectors.len() {
        return Err(io::Error::other(
            "population durations selector count mismatch",
        ));
    }
    for selector in &population.selectors {
        if !durations.contains_key(selector) {
            return Err(io::Error::other(
                "population durations missing selector",
            ));
        }
    }
    let payload = PopulationDurationsFile {
        schema_version: POPULATION_DURATIONS_SCHEMA.to_string(),
        cache_schema_version: CACHE_SCHEMA_VERSION.to_string(),
        generation_fingerprint: population.generation_fingerprint.clone(),
        input_fingerprint: population.input_fingerprint.clone(),
        entries_fingerprint: population.entries_fingerprint.clone(),
        durations,
    };
    let path = population_durations_path(cache_root);
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("population_durations path has no parent"))?;
    let tmp = parent.join(format!(
        ".population_durations.{}.tmp",
        rust_cov_unique_suffix()
    ));
    kiss_publication_barrier::publish_atomically("rust_population_durations", &path, &tmp, |file| {
        serde_json::to_writer(&mut *file, &payload).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        Ok(())
    })
}

/// Best-effort publish after population write. Failures leave the warm path to rebuild.
pub(crate) fn try_publish_durations_after_population(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selectors: &[String],
    entries_fingerprint: &str,
) {
    let population = RustPopulationState {
        input_fingerprint: identity.input_digest.clone(),
        generation_fingerprint: identity.generation_fingerprint.clone(),
        selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        entries_fingerprint: entries_fingerprint.to_string(),
        selectors: selectors.to_vec(),
        line_index: BTreeMap::new(),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let Some(pairs) =
        load_durations_from_entries(cache_root, &population, identity, req, tools)
    else {
        return;
    };
    let _ = write_population_durations(cache_root, &population, &pairs);
}

pub(crate) fn load_durations_from_entries(
    cache_root: &Path,
    population: &RustPopulationState,
    identity: &RustCoverageBatchIdentity,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Option<Vec<(String, Duration)>> {
    let mut out = Vec::with_capacity(population.selectors.len());
    for selector in &population.selectors {
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
        let entry = load_rust_cov_cache_entry(cache_root, &fingerprint)?;
        if entry.selector != *selector {
            return None;
        }
        if entry.generation_fingerprint != population.generation_fingerprint {
            return None;
        }
        out.push((selector.clone(), entry.duration));
    }
    Some(out)
}

fn duration_as_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "batch_population_durations_test.rs"]
mod tests;
