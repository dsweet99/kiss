use crate::batch_derived_index::read_population_generation;
use crate::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{
    RustCovCacheEntry, create_new_cache_file, generation_entries_fingerprint,
    load_rust_cov_cache_entry, repo_relative_coverage_file, rust_cov_unique_suffix,
};
use crate::{CACHE_SCHEMA_VERSION, RustLineCoverage, RustLlvmCovError};
use rpytest_runner::TestStatus;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
pub const INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v2";
pub const POPULATION_SCHEMA_VERSION: &str = "rust-llvm-cov-population-v4";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedPublishCounters {
    pub derived_repair: bool,
    pub entry_generation_count: usize,
    pub current_index_generation: String,
    pub cache_pruned_entries: usize,
}

type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;
pub(crate) fn try_publish_population_derived_state(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    population_selectors: &[String],
) -> Result<Option<DerivedPublishCounters>, RustLlvmCovError> {
    let Some(selectors) = req.population_publication_selectors.as_deref() else {
        return Ok(None);
    };
    if selectors.is_empty() {
        return Ok(None);
    }
    let mut manifest_selectors = population_selectors.to_vec();
    manifest_selectors.sort();
    manifest_selectors.dedup();
    if manifest_selectors != selectors {
        return Ok(None);
    }
    let repair = derived_state_stale(req, tools, identity, selectors)?;
    if !repair && all_entries_hit(req, tools, identity)? {
        return Ok(None);
    }
    publish_derived_state(req, tools, identity, selectors, repair).map(Some)
}
pub fn population_derived_state_stale(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<bool, RustLlvmCovError> {
    let Some(selectors) = req.population_publication_selectors.as_deref() else {
        return Ok(false);
    };
    derived_state_stale(req, tools, identity, selectors)
}
fn derived_state_stale(
    req: &RustCoverageBatchRequest,
    _tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
) -> Result<bool, RustLlvmCovError> {
    Ok(!population_state_is_current(req, identity, selectors)?)
}
fn population_state_is_current(
    req: &RustCoverageBatchRequest,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
) -> Result<bool, RustLlvmCovError> {
    population_manifest_state_is_current(&req.cache_root, &req.source_root, identity, selectors)
}
pub fn population_manifest_state_is_current(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
) -> Result<bool, RustLlvmCovError> {
    Ok(crate::batch_derived_index::load_current_population_state(
        cache_root,
        source_root,
        identity,
        Some(selectors),
    )
    .is_some())
}
fn all_entries_hit(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<bool, RustLlvmCovError> {
    for selector in selectors_for_publication(req)? {
        let fingerprint = crate::batch_fingerprint::entry_fingerprint(
            &identity.input_digest,
            req,
            tools,
            selector,
        );
        if load_rust_cov_cache_entry(&req.cache_root, &fingerprint).is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}
fn selectors_for_publication(
    req: &RustCoverageBatchRequest,
) -> Result<&[String], RustLlvmCovError> {
    req.population_publication_selectors
        .as_deref()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("missing population selectors".into()))
}
pub fn publish_derived_state(
    req: &RustCoverageBatchRequest,
    _tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    derived_repair: bool,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    let pruned = prune_non_current_generations(&req.cache_root, &identity.generation_fingerprint)?;
    let index = build_generation_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
    )?;
    let entries_fingerprint =
        generation_entries_fingerprint(&req.cache_root, &identity.generation_fingerprint)
            .map_err(RustLlvmCovError::Io)?;
    write_coverage_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
        &entries_fingerprint,
        &index,
    )?;
    write_population_manifest(
        &req.cache_root,
        &req.source_root,
        identity,
        selectors,
        &entries_fingerprint,
    )?;
    Ok(DerivedPublishCounters {
        derived_repair,
        entry_generation_count: count_generations(&req.cache_root)?,
        current_index_generation: identity.generation_fingerprint.clone(),
        cache_pruned_entries: pruned,
    })
}

fn prune_non_current_generations(
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

pub(crate) fn derived_generation_line_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<RustCoverageIndex, RustLlvmCovError> {
    build_generation_index(cache_root, source_root, generation)
}

fn build_generation_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
) -> Result<RustCoverageIndex, RustLlvmCovError> {
    let mut files: RustCoverageIndex = BTreeMap::new();
    let entries_dir = cache_root.join("entries");
    if !entries_dir.is_dir() {
        return Ok(files);
    }
    for entry in fs::read_dir(&entries_dir).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some((selector, status, coverage)) = load_index_entry(&path, generation) else {
            continue;
        };
        if status != TestStatus::Passed || coverage.files.is_empty() {
            continue;
        }
        for file in coverage.files.keys() {
            if let Some(rel) = repo_relative_coverage_file(source_root, file) {
                files.entry(rel).or_default().insert(selector.clone());
            }
        }
    }
    Ok(files)
}

fn load_index_entry(
    path: &Path,
    generation: &str,
) -> Option<(String, TestStatus, RustLineCoverage)> {
    let bytes = fs::read(path).ok()?;
    let entry: RustCovCacheEntry = serde_json::from_slice(&bytes).ok()?;
    if entry.generation_fingerprint != generation {
        return None;
    }
    Some((entry.selector, entry.status, entry.coverage))
}

fn count_generations(cache_root: &Path) -> Result<usize, RustLlvmCovError> {
    let entries_dir = cache_root.join("entries");
    if !entries_dir.is_dir() {
        return Ok(0);
    }
    let mut generations = BTreeSet::new();
    for entry in fs::read_dir(&entries_dir).map_err(RustLlvmCovError::Io)? {
        let path = entry.map_err(RustLlvmCovError::Io)?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(RustLlvmCovError::Io)?;
        let Ok(parsed): Result<RustCovCacheEntry, _> = serde_json::from_slice(&bytes) else {
            continue;
        };
        if !parsed.generation_fingerprint.is_empty() {
            generations.insert(parsed.generation_fingerprint);
        }
    }
    Ok(generations.len())
}

fn write_coverage_index(
    cache_root: &Path,
    source_root: &Path,
    generation: &str,
    entries_fingerprint: &str,
    index: &RustCoverageIndex,
) -> Result<(), RustLlvmCovError> {
    #[derive(Serialize)]
    struct OnDiskIndex<'a> {
        schema_version: &'a str,
        source_root: String,
        generation_fingerprint: &'a str,
        entries_fingerprint: &'a str,
        files: &'a RustCoverageIndex,
    }

    let path = cache_root.join("index.json");
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("index path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp_path = parent.join(format!(".index.{}.tmp", rust_cov_unique_suffix()));
    let mut file = create_new_cache_file(&tmp_path).map_err(RustLlvmCovError::Io)?;
    let payload = OnDiskIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        source_root: source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf())
            .to_string_lossy()
            .to_string(),
        generation_fingerprint: generation,
        entries_fingerprint,
        files: index,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to write index json: {err}"))
    })?;
    file.write_all(b"\n").map_err(RustLlvmCovError::Io)?;
    file.sync_all().map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(tmp_path, path).map_err(RustLlvmCovError::Io)
}

fn write_population_manifest(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    entries_fingerprint: &str,
) -> Result<(), RustLlvmCovError> {
    #[derive(Serialize)]
    struct OrdinarySourceDigestRecord<'a> {
        path: &'a str,
        digest: &'a str,
    }

    #[derive(Serialize)]
    struct PopulationManifest<'a> {
        schema_version: &'a str,
        cache_schema_version: &'a str,
        source_root: String,
        generation_fingerprint: &'a str,
        input_fingerprint: &'a str,
        selection_context_fingerprint: &'a str,
        entries_fingerprint: &'a str,
        selectors: &'a [String],
        ordinary_source_digests: Vec<OrdinarySourceDigestRecord<'a>>,
    }

    let path = cache_root.join("population.json");
    let parent = path
        .parent()
        .ok_or_else(|| RustLlvmCovError::InvalidRequest("population path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(RustLlvmCovError::Io)?;
    let tmp_path = parent.join(format!(".population.{}.tmp", rust_cov_unique_suffix()));
    let mut file = create_new_cache_file(&tmp_path).map_err(RustLlvmCovError::Io)?;
    let ordinary_source_digests = identity
        .ordinary_source_digests
        .iter()
        .map(|(path, digest)| OrdinarySourceDigestRecord {
            path: path.as_str(),
            digest: digest.as_str(),
        })
        .collect();
    let payload = PopulationManifest {
        schema_version: POPULATION_SCHEMA_VERSION,
        cache_schema_version: CACHE_SCHEMA_VERSION,
        source_root: source_root
            .canonicalize()
            .unwrap_or_else(|_| source_root.to_path_buf())
            .to_string_lossy()
            .to_string(),
        generation_fingerprint: &identity.generation_fingerprint,
        input_fingerprint: &identity.input_digest,
        selection_context_fingerprint: &identity.selection_context_fingerprint,
        entries_fingerprint,
        selectors,
        ordinary_source_digests,
    };
    serde_json::to_writer_pretty(&mut file, &payload).map_err(|err| {
        RustLlvmCovError::InvalidRequest(format!("failed to write index json: {err}"))
    })?;
    file.write_all(b"\n").map_err(RustLlvmCovError::Io)?;
    file.sync_all().map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(tmp_path, path).map_err(RustLlvmCovError::Io)
}

#[cfg(test)]
#[path = "batch_derived_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_derived_selective_test.rs"]
mod selective_tests;

#[cfg(test)]
#[path = "batch_derived_real_cache_probe_test.rs"]
mod real_cache_probe_tests;

#[cfg(test)]
#[path = "batch_derived_state_test.rs"]
mod state_tests;
