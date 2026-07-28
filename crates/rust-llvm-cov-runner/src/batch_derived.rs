pub(crate) use crate::batch_derived_prune::maybe_prune_obsolete_selective_after_batch;
use crate::batch_derived_prune::prune_non_current_generations;
pub use crate::batch_derived_prune::prune_obsolete_selective_generations;
use crate::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{
    RustCovCacheEntry, create_new_cache_file, generation_entries_fingerprint,
    load_rust_cov_cache_entry, repo_relative_coverage_file, rust_cov_unique_suffix,
};
use crate::{
    RustLineCoverage, RustLlvmCovError, RustTestBinaryIdentity,
    batch_check_aggregate::ValidatedCheckAggregate,
};
use rpytest_runner::TestStatus;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
pub const INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v3";
pub const POPULATION_SCHEMA_VERSION: &str = "rust-llvm-cov-population-v5";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedPublishCounters {
    pub derived_repair: bool,
    pub entry_generation_count: usize,
    pub current_index_generation: String,
    pub cache_pruned_entries: usize,
}

type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;
#[cfg(test)]
pub(crate) fn try_publish_population_derived_state(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    population_selectors: &[String],
) -> Result<Option<DerivedPublishCounters>, RustLlvmCovError> {
    try_publish_population_derived_state_with_binaries(
        req,
        tools,
        identity,
        population_selectors,
        &legacy_test_binaries(population_selectors),
    )
}

pub(crate) fn try_publish_population_derived_state_with_binaries(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    population_selectors: &[String],
    test_binaries: &[RustTestBinaryIdentity],
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
    publish_derived_state_with_binaries(req, tools, identity, selectors, test_binaries, repair)
        .map(Some)
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
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    derived_repair: bool,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    publish_derived_state_with_binaries(
        req,
        tools,
        identity,
        selectors,
        &legacy_test_binaries(selectors),
        derived_repair,
    )
}

pub fn publish_derived_state_with_binaries(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    test_binaries: &[RustTestBinaryIdentity],
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
    crate::batch_derived_manifest::write_population_manifest(
        &req.cache_root,
        &req.source_root,
        identity,
        selectors,
        test_binaries,
        &entries_fingerprint,
    )?;
    let _ = crate::batch_identity_seal::write_identity_mtime_seal(
        &req.cache_root,
        &req.source_root,
        req,
        tools,
        identity,
    );
    Ok(DerivedPublishCounters {
        derived_repair,
        entry_generation_count: count_generations(&req.cache_root)?,
        current_index_generation: identity.generation_fingerprint.clone(),
        cache_pruned_entries: pruned,
    })
}

pub(crate) fn publish_conservative_derived_state_from_check_aggregate(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    aggregate: &ValidatedCheckAggregate,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    let pruned = prune_non_current_generations(&req.cache_root, &identity.generation_fingerprint)?;
    let selector_set = aggregate.selectors.iter().cloned().collect::<BTreeSet<_>>();
    let index = aggregate
        .aggregate_covered_lines
        .keys()
        .map(|file| (file.clone(), selector_set.clone()))
        .collect::<RustCoverageIndex>();
    let entries_fingerprint =
        crate::batch_derived_index::check_aggregate_entries_fingerprint(aggregate);
    write_coverage_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
        &entries_fingerprint,
        &index,
    )?;
    let test_binaries = aggregate
        .binaries
        .values()
        .map(|record| RustTestBinaryIdentity {
            id: record.id.clone(),
            executable: record.executable.clone(),
            digest: record.digest.clone(),
        })
        .collect::<Vec<_>>();
    crate::batch_derived_manifest::write_population_manifest(
        &req.cache_root,
        &req.source_root,
        identity,
        &aggregate.selectors,
        &test_binaries,
        &entries_fingerprint,
    )?;
    let _ = crate::batch_identity_seal::write_identity_mtime_seal(
        &req.cache_root,
        &req.source_root,
        req,
        tools,
        identity,
    );
    Ok(DerivedPublishCounters {
        derived_repair: false,
        entry_generation_count: count_generations(&req.cache_root)?,
        current_index_generation: identity.generation_fingerprint.clone(),
        cache_pruned_entries: pruned,
    })
}

fn legacy_test_binaries(selectors: &[String]) -> Vec<RustTestBinaryIdentity> {
    if selectors.is_empty() {
        return Vec::new();
    }
    vec![RustTestBinaryIdentity {
        id: "test-bin".to_string(),
        executable: "test-bin".to_string(),
        digest: "0000000000000000".to_string(),
    }]
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
    kiss_publication_barrier::after_sync_before_rename("rust_derived_index", &tmp_path, &path)
        .map_err(RustLlvmCovError::Io)?;
    drop(file);
    fs::rename(&tmp_path, &path).map_err(RustLlvmCovError::Io)?;
    kiss_publication_barrier::after_rename("rust_derived_index", &tmp_path, &path)
        .map_err(RustLlvmCovError::Io)
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
