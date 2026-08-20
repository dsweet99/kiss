use crate::plan::batch_fingerprint::RustCoverageBatchIdentity;
use crate::publish_derived::batch_derived::{INDEX_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION};
use crate::publish_derived::batch_derived_index_check_aggregate_support::{
    CHECK_AGGREGATE_ENTRIES_PREFIX, index_matches_check_aggregate,
    load_check_aggregate_population_state,
};
use crate::publish_derived::batch_derived_index_reverse::reverse_bound_index_ok;
#[cfg(test)]
use crate::publish_derived::batch_derived_index_types::OnDiskIndex;
use crate::publish_derived::batch_derived_index_types::{
    OnDiskIndexWithFiles, PopulationManifestOnDisk, PopulationManifestRaw, RustCoverageIndex,
    validate_ordinary_source_digests, validate_test_binaries,
};
use crate::rust_cov_cache::generation_entries_fingerprint;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) use crate::publish_derived::batch_derived_index_check_aggregate_support::check_aggregate_entries_fingerprint;
pub use crate::publish_derived::batch_derived_index_check_aggregate_support::is_check_aggregate_population;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustPopulationState {
    pub input_fingerprint: String,
    pub generation_fingerprint: String,
    pub selection_context_fingerprint: String,
    pub entries_fingerprint: String,
    pub selectors: Vec<String>,
    pub line_index: RustCoverageIndex,
    pub ordinary_source_digests: BTreeMap<String, String>,
    pub test_binaries: BTreeMap<String, crate::RustTestBinaryIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustGenerationCoverageSnapshot {
    pub identity: String,
    pub covered_lines: BTreeMap<String, BTreeSet<u32>>,
    pub population: RustPopulationState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RustSnapshotDelta {
    Unchanged,
    Modified(Vec<PathBuf>),
    StructuralChange,
}
pub fn load_current_population_state(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<RustPopulationState> {
    load_validated_population_state(
        cache_root,
        source_root,
        selectors,
        PopulationLoadMode::Current {
            input_fingerprint: identity.input_digest.clone(),
            generation_fingerprint: identity.generation_fingerprint.clone(),
            selection_context_fingerprint: identity.selection_context_fingerprint.clone(),
        },
    )
}

pub fn load_current_generation_coverage_snapshot(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<RustGenerationCoverageSnapshot> {
    let population = load_current_population_state(cache_root, source_root, identity, selectors)?;
    let entries = crate::publish_derived::batch_derived_snapshot::load_manifest_generation_entries(
        cache_root,
        source_root,
        &population,
    )?;
    let snapshot_identity =
        crate::publish_derived::batch_derived_snapshot::stable_generation_coverage_identity(
            &population,
            &entries,
        );
    Some(RustGenerationCoverageSnapshot {
        identity: snapshot_identity,
        covered_lines: entries,
        population,
    })
}
pub fn load_reusable_prior_population_state(
    cache_root: &Path,
    source_root: &Path,
    selectors: Option<&[String]>,
    selection_context_fingerprint: &str,
) -> Option<RustPopulationState> {
    load_validated_population_state(
        cache_root,
        source_root,
        selectors,
        PopulationLoadMode::ReusablePrior {
            selection_context_fingerprint: selection_context_fingerprint.to_string(),
        },
    )
}
pub(crate) enum PopulationLoadMode {
    Current {
        input_fingerprint: String,
        generation_fingerprint: String,
        selection_context_fingerprint: String,
    },
    ReusablePrior {
        selection_context_fingerprint: String,
    },
}
fn load_validated_population_state(
    cache_root: &Path,
    source_root: &Path,
    selectors: Option<&[String]>,
    mode: PopulationLoadMode,
) -> Option<RustPopulationState> {
    let manifest = read_population_manifest(cache_root)?;
    if manifest
        .entries_fingerprint
        .starts_with(CHECK_AGGREGATE_ENTRIES_PREFIX)
    {
        return load_check_aggregate_population_state(
            cache_root,
            source_root,
            selectors,
            mode,
            manifest,
        );
    }
    let index = read_index_with_files(cache_root)?;
    if !population_artifacts_compatible(cache_root, source_root, &index, &manifest, selectors) {
        return None;
    }
    let selection_context_fingerprint = population_selection_context_matches(&manifest, &mode)?;
    if !population_current_identity_matches(&manifest, &mode) {
        return None;
    }
    if !manifest_generation_entries_complete(cache_root, &manifest) {
        return None;
    }
    Some(RustPopulationState {
        input_fingerprint: manifest.input_fingerprint,
        generation_fingerprint: manifest.generation_fingerprint,
        selection_context_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index: index.files,
        ordinary_source_digests: manifest.ordinary_source_digests,
        test_binaries: manifest.test_binaries,
    })
}

fn population_artifacts_compatible(
    cache_root: &Path,
    source_root: &Path,
    index: &OnDiskIndexWithFiles,
    manifest: &PopulationManifestOnDisk,
    selectors: Option<&[String]>,
) -> bool {
    if index.source_root != normalized_source_root(source_root) {
        return false;
    }
    if manifest.schema_version != POPULATION_SCHEMA_VERSION
        || index.schema_version != INDEX_SCHEMA_VERSION
    {
        return false;
    }
    if let Some(selectors) = selectors {
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        if manifest.selectors != expected {
            return false;
        }
    }
    if index.generation_fingerprint != manifest.generation_fingerprint
        || index.entries_fingerprint != manifest.entries_fingerprint
    {
        return false;
    }
    if manifest
        .entries_fingerprint
        .starts_with(CHECK_AGGREGATE_ENTRIES_PREFIX)
    {
        return index_matches_check_aggregate(cache_root, source_root, manifest, index);
    }
    reverse_bound_index_ok(cache_root, manifest, index)
        || (generation_entries_fingerprint(cache_root, &manifest.generation_fingerprint)
            .ok()
            .is_some_and(|computed| computed == manifest.entries_fingerprint)
            && index_matches_derived_entries(cache_root, source_root, manifest, index))
}

fn index_matches_derived_entries(
    cache_root: &Path,
    source_root: &Path,
    manifest: &PopulationManifestOnDisk,
    index: &OnDiskIndexWithFiles,
) -> bool {
    crate::publish_derived::batch_derived::derived_generation_line_index(
        cache_root,
        source_root,
        &manifest.generation_fingerprint,
    )
    .ok()
    .is_some_and(|derived| derived == index.files)
}

pub(crate) fn population_selection_context_matches(
    manifest: &PopulationManifestOnDisk,
    mode: &PopulationLoadMode,
) -> Option<String> {
    let expected = match mode {
        PopulationLoadMode::Current {
            selection_context_fingerprint,
            ..
        }
        | PopulationLoadMode::ReusablePrior {
            selection_context_fingerprint,
        } => selection_context_fingerprint,
    };
    (manifest.selection_context_fingerprint == *expected).then(|| expected.clone())
}

pub(crate) fn population_current_identity_matches(
    manifest: &PopulationManifestOnDisk,
    mode: &PopulationLoadMode,
) -> bool {
    match mode {
        PopulationLoadMode::Current {
            input_fingerprint,
            generation_fingerprint,
            ..
        } => {
            manifest.input_fingerprint == *input_fingerprint
                && manifest.generation_fingerprint == *generation_fingerprint
        }
        PopulationLoadMode::ReusablePrior { .. } => true,
    }
}

fn manifest_generation_entries_complete(
    cache_root: &Path,
    manifest: &PopulationManifestOnDisk,
) -> bool {
    if manifest
        .entries_fingerprint
        .starts_with(CHECK_AGGREGATE_ENTRIES_PREFIX)
    {
        return true;
    }
    if let Some(reverse) = manifest.reverse_line_index.as_ref() {
        let Some(state) = crate::publish_derived::batch_entry_state::read_entry_state(cache_root)
        else {
            return false;
        };
        return crate::publish_derived::batch_entry_state::entry_state_matches(
            &state,
            &manifest.generation_fingerprint,
            &manifest.entries_fingerprint,
            reverse.entry_state_revision,
        );
    }
    manifest_generation_entries_scanned(cache_root, manifest)
}

fn manifest_generation_entries_scanned(
    cache_root: &Path,
    manifest: &PopulationManifestOnDisk,
) -> bool {
    let entries_dir = cache_root.join("entries");
    if !entries_dir.is_dir() {
        return false;
    }
    let mut seen = BTreeSet::new();
    let Ok(entries) = fs::read_dir(&entries_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_slice::<crate::RustCovCacheEntry>(&bytes) else {
            continue;
        };
        if parsed.schema_version != crate::CACHE_SCHEMA_VERSION
            || parsed.generation_fingerprint != manifest.generation_fingerprint
        {
            continue;
        }
        if parsed.test_binary_ids.is_empty()
            || parsed
                .test_binary_ids
                .iter()
                .any(|id| !manifest.test_binaries.contains_key(id))
        {
            return false;
        }
        if !seen.insert(parsed.selector.clone()) {
            return false;
        }
    }
    let mut expected = manifest.selectors.clone();
    expected.sort();
    expected.dedup();
    let mut actual: Vec<_> = seen.into_iter().collect();
    actual.sort();
    actual == expected
}
pub fn load_current_generation_line_index(
    cache_root: &Path,
    source_root: &Path,
) -> Option<RustCoverageIndex> {
    let manifest = read_population_manifest(cache_root)?;
    let index = read_index_with_files(cache_root)?;
    if !population_artifacts_compatible(cache_root, source_root, &index, &manifest, None) {
        return None;
    }
    Some(index.files)
}

pub fn reusable_snapshot_delta(
    source_root: &Path,
    prior: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> RustSnapshotDelta {
    crate::publish_derived::batch_derived_snapshot::reusable_snapshot_delta(
        source_root,
        prior,
        current,
    )
}

pub(crate) use crate::rust_cov_cache::normalized_source_root;

fn read_index_with_files(cache_root: &Path) -> Option<OnDiskIndexWithFiles> {
    let bytes = fs::read(cache_root.join("index.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
pub(crate) fn read_coverage_index(cache_root: &Path) -> Option<OnDiskIndex> {
    let bytes = fs::read(cache_root.join("index.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn read_population_manifest(cache_root: &Path) -> Option<PopulationManifestOnDisk> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    let raw: PopulationManifestRaw = serde_json::from_slice(&bytes).ok()?;
    if raw.schema_version != POPULATION_SCHEMA_VERSION {
        return None;
    }
    let ordinary_source_digests = validate_ordinary_source_digests(raw.ordinary_source_digests)?;
    let test_binaries = validate_test_binaries(raw.test_binaries)?;
    Some(PopulationManifestOnDisk {
        schema_version: raw.schema_version,
        generation_fingerprint: raw.generation_fingerprint,
        input_fingerprint: raw.input_fingerprint,
        selection_context_fingerprint: raw.selection_context_fingerprint,
        entries_fingerprint: raw.entries_fingerprint,
        selectors: raw.selectors,
        ordinary_source_digests,
        test_binaries,
        reverse_line_index: raw.reverse_line_index,
    })
}

pub(crate) fn read_population_generation(cache_root: &Path) -> Option<String> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("generation_fingerprint")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
#[path = "batch_derived_index_check_aggregate_test.rs"]
mod check_aggregate_tests;

#[cfg(test)]
#[path = "batch_derived_index_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_derived_index_reject_test.rs"]
mod reject_tests;

#[cfg(test)]
#[path = "batch_derived_index_reusable_test.rs"]
mod reusable_tests;
