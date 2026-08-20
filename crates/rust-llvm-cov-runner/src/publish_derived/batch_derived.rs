use crate::plan::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::publish_derived::batch_derived_generations::{
    build_generation_index, count_generations,
};
pub(crate) use crate::publish_derived::batch_derived_prune::maybe_prune_obsolete_selective_after_batch;
use crate::publish_derived::batch_derived_prune::prune_non_current_generations;
pub use crate::publish_derived::batch_derived_prune::prune_obsolete_selective_generations;
use crate::rust_cov_cache::{generation_entries_fingerprint, load_rust_cov_cache_entry};
use crate::{
    RustLlvmCovError, RustTestBinaryIdentity, batch_check_aggregate::ValidatedCheckAggregate,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;

#[cfg(test)]
use crate::RustLineCoverage;
#[cfg(test)]
use crate::rust_cov_cache::RustCovCacheEntry;
#[cfg(test)]
use rpytest_runner::TestStatus;
pub const INDEX_SCHEMA_VERSION: &str = "rust-llvm-cov-index-v3";
pub const POPULATION_SCHEMA_VERSION: &str = "rust-llvm-cov-population-v6";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedPublishCounters {
    pub derived_repair: bool,
    pub entry_generation_count: usize,
    pub current_index_generation: String,
    pub cache_pruned_entries: usize,
    pub reverse_published: bool,
    pub reverse_snapshots_reclaimed: usize,
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
        return republish_if_entry_state_missing(req, tools, identity, test_binaries);
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
        return republish_if_entry_state_missing(req, tools, identity, test_binaries);
    }
    publish_derived_state_with_binaries(req, tools, identity, selectors, test_binaries, repair)
        .map(Some)
}

fn republish_if_entry_state_missing(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    test_binaries: &[RustTestBinaryIdentity],
) -> Result<Option<DerivedPublishCounters>, RustLlvmCovError> {
    if crate::publish_derived::batch_entry_state::read_entry_state(&req.cache_root).is_some() {
        return Ok(None);
    }
    let Ok(bytes) = fs::read(req.cache_root.join("population.json")) else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(None);
    };
    let selectors = value
        .get("reverse_line_index")
        .and_then(|_| value.get("selectors"))
        .and_then(|node| node.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty());
    let Some(selectors) = selectors else {
        return Ok(None);
    };
    publish_derived_state_with_binaries(req, tools, identity, &selectors, test_binaries, true)
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
    Ok(
        crate::publish_derived::batch_derived_index::load_current_population_state(
            cache_root,
            source_root,
            identity,
            Some(selectors),
        )
        .is_some(),
    )
}
fn all_entries_hit(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<bool, RustLlvmCovError> {
    for selector in selectors_for_publication(req)? {
        let fingerprint = crate::plan::batch_fingerprint::entry_fingerprint(
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
    crate::publish_derived::batch_publication_tmp::sweep_orphaned_publication_tmps(&req.cache_root)
        .map_err(|err| io_context("sweep_orphaned_publication_tmps", err))?;
    let pruned = prune_non_current_generations(&req.cache_root, &identity.generation_fingerprint)
        .map_err(|err| annotate_io("prune_non_current_generations", err))?;
    let index = build_generation_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
    )
    .map_err(|err| annotate_io("build_generation_index", err))?;
    let entries_fingerprint =
        generation_entries_fingerprint(&req.cache_root, &identity.generation_fingerprint)
            .map_err(|err| io_context("generation_entries_fingerprint", err))?;
    let prior_snapshot =
        crate::publish_derived::batch_reverse_line_index::read_prior_snapshot_id(&req.cache_root);
    let revision = crate::publish_derived::batch_entry_state::publish_next_entry_state(
        &req.cache_root,
        &identity.generation_fingerprint,
        &entries_fingerprint,
    )
    .map_err(|err| annotate_io("publish_next_entry_state", err))?;
    let reverse = crate::publish_derived::batch_reverse_line_index::publish_reverse_line_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
        &entries_fingerprint,
        revision,
    )
    .map_err(|err| annotate_io("publish_reverse_line_index", err))?;
    crate::publish_derived::batch_derived_index_write::write_coverage_index(
        &req.cache_root,
        &req.source_root,
        &identity.generation_fingerprint,
        &entries_fingerprint,
        &index,
    )
    .map_err(|err| annotate_io("write_coverage_index", err))?;
    crate::publish_derived::batch_derived_manifest::write_population_and_durations(
        req,
        tools,
        identity,
        selectors,
        test_binaries,
        &entries_fingerprint,
        Some(&reverse),
    )
    .map_err(|err| annotate_io("write_population_and_durations", err))?;
    let reverse_snapshots_reclaimed =
        match crate::publish_derived::batch_reverse_line_index::prune_unreferenced_snapshots(
            &req.cache_root,
            &reverse.snapshot_id,
            prior_snapshot.as_deref(),
        ) {
            Ok(removed) => removed,
            Err(err) => {
                eprintln!(
                    "kiss: reverse snapshot prune failed (active snapshot retained): {err:?}"
                );
                0
            }
        };
    let _ = crate::plan::batch_identity_seal::write_identity_mtime_seal(
        &req.cache_root,
        &req.source_root,
        req,
        tools,
        identity,
    );
    Ok(DerivedPublishCounters {
        derived_repair,
        entry_generation_count: count_generations(&req.cache_root)
            .map_err(|err| annotate_io("count_generations", err))?,
        current_index_generation: identity.generation_fingerprint.clone(),
        cache_pruned_entries: pruned,
        reverse_published: true,
        reverse_snapshots_reclaimed,
    })
}

fn io_context(step: &str, err: io::Error) -> RustLlvmCovError {
    RustLlvmCovError::Io(io::Error::new(err.kind(), format!("{step}: {err}")))
}

fn annotate_io(step: &str, err: RustLlvmCovError) -> RustLlvmCovError {
    match err {
        RustLlvmCovError::Io(inner) => io_context(step, inner),
        other => other,
    }
}

pub(crate) fn publish_conservative_derived_state_from_check_aggregate(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    aggregate: &ValidatedCheckAggregate,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    let pruned = prune_non_current_generations(&req.cache_root, &identity.generation_fingerprint)?;

    let index = aggregate
        .aggregate_covered_lines
        .keys()
        .map(|file| (file.clone(), BTreeSet::new()))
        .collect::<RustCoverageIndex>();
    let entries_fingerprint =
        crate::publish_derived::batch_derived_index::check_aggregate_entries_fingerprint(aggregate);
    crate::publish_derived::batch_derived_index_write::write_coverage_index(
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
    crate::publish_derived::batch_derived_manifest::write_population_and_durations(
        req,
        tools,
        identity,
        &aggregate.selectors,
        &test_binaries,
        &entries_fingerprint,
        None,
    )?;
    let _ = crate::plan::batch_identity_seal::write_identity_mtime_seal(
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
        reverse_published: false,
        reverse_snapshots_reclaimed: 0,
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
