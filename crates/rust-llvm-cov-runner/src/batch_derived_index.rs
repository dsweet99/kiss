use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::CACHE_SCHEMA_VERSION;
use crate::batch_derived::{INDEX_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION};
use crate::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_cov_cache::{RustCovCacheEntry, generation_entries_fingerprint};
use serde::Deserialize;
type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustPopulationState {
    pub input_fingerprint: String,
    pub generation_fingerprint: String,
    pub selection_context_fingerprint: String,
    pub entries_fingerprint: String,
    pub selectors: Vec<String>,
    pub line_index: RustCoverageIndex,
    pub ordinary_source_digests: BTreeMap<String, String>,
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
enum PopulationLoadMode {
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
    if generation_entries_fingerprint(cache_root, &manifest.generation_fingerprint)
        .ok()
        .is_none_or(|computed| computed != manifest.entries_fingerprint)
    {
        return false;
    }
    index_matches_derived_entries(cache_root, source_root, manifest, index)
}

fn index_matches_derived_entries(
    cache_root: &Path,
    source_root: &Path,
    manifest: &PopulationManifestOnDisk,
    index: &OnDiskIndexWithFiles,
) -> bool {
    crate::batch_derived::derived_generation_line_index(
        cache_root,
        source_root,
        &manifest.generation_fingerprint,
    )
    .ok()
    .is_some_and(|derived| derived == index.files)
}

fn population_selection_context_matches(
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

fn population_current_identity_matches(
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
        let Ok(parsed) = serde_json::from_slice::<RustCovCacheEntry>(&bytes) else {
            continue;
        };
        if parsed.schema_version != CACHE_SCHEMA_VERSION
            || parsed.generation_fingerprint != manifest.generation_fingerprint
        {
            continue;
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
    // Share population-loader artifact validation, including entry-derived
    // index agreement, so callers cannot observe a tampered index.files map.
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
    if prior.keys().ne(current.keys()) {
        return RustSnapshotDelta::StructuralChange;
    }
    let modified = prior
        .iter()
        .filter(|(path, digest)| current.get(*path) != Some(*digest))
        .map(|(path, _digest)| source_root.join(path))
        .collect::<Vec<_>>();
    if modified.is_empty() {
        RustSnapshotDelta::Unchanged
    } else {
        RustSnapshotDelta::Modified(modified)
    }
}

fn normalized_source_root(source_root: &Path) -> String {
    source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn read_index_with_files(cache_root: &Path) -> Option<OnDiskIndexWithFiles> {
    let bytes = fs::read(cache_root.join("index.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[derive(Deserialize)]
pub(crate) struct OnDiskIndexWithFiles {
    pub(crate) schema_version: String,
    pub(crate) source_root: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) files: RustCoverageIndex,
}

#[cfg(test)]
pub(crate) fn read_coverage_index(cache_root: &Path) -> Option<OnDiskIndex> {
    let bytes = fs::read(cache_root.join("index.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
#[derive(Deserialize)]
pub(crate) struct OnDiskIndex {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) entries_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct OrdinarySourceDigestRecord {
    pub(crate) path: String,
    pub(crate) digest: String,
}

#[derive(Deserialize)]
struct PopulationManifestRaw {
    schema_version: String,
    generation_fingerprint: String,
    input_fingerprint: String,
    selection_context_fingerprint: String,
    entries_fingerprint: String,
    selectors: Vec<String>,
    ordinary_source_digests: Vec<OrdinarySourceDigestRecord>,
}

pub(crate) struct PopulationManifestOnDisk {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) selection_context_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) ordinary_source_digests: BTreeMap<String, String>,
}

pub(crate) fn read_population_manifest(cache_root: &Path) -> Option<PopulationManifestOnDisk> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    let raw: PopulationManifestRaw = serde_json::from_slice(&bytes).ok()?;
    if raw.schema_version != POPULATION_SCHEMA_VERSION {
        return None;
    }
    let ordinary_source_digests = validate_ordinary_source_digests(raw.ordinary_source_digests)?;
    Some(PopulationManifestOnDisk {
        schema_version: raw.schema_version,
        generation_fingerprint: raw.generation_fingerprint,
        input_fingerprint: raw.input_fingerprint,
        selection_context_fingerprint: raw.selection_context_fingerprint,
        entries_fingerprint: raw.entries_fingerprint,
        selectors: raw.selectors,
        ordinary_source_digests,
    })
}

fn validate_ordinary_source_digests(
    records: Vec<OrdinarySourceDigestRecord>,
) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    let mut previous: Option<String> = None;
    for record in records {
        if previous.as_ref().is_some_and(|prev| record.path <= *prev) {
            return None;
        }
        if !valid_ordinary_source_path(&record.path)
            || !valid_ordinary_source_digest(&record.digest)
        {
            return None;
        }
        if out.insert(record.path.clone(), record.digest).is_some() {
            return None;
        }
        previous = Some(record.path);
    }
    Some(out)
}

fn valid_ordinary_source_path(path: &str) -> bool {
    if path.is_empty() || !(path.ends_with(".rs") || path.ends_with(".inc")) || path.contains('\\')
    {
        return false;
    }
    let path = Path::new(path);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_ordinary_source_digest(digest: &str) -> bool {
    digest.len() == 16
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
#[path = "batch_derived_index_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_derived_index_reject_test.rs"]
mod reject_tests;

#[cfg(test)]
#[path = "batch_derived_index_reusable_test.rs"]
mod reusable_tests;

#[cfg(test)]
mod coverage_witness {
    use std::collections::BTreeMap;

    use super::{INDEX_SCHEMA_VERSION, OnDiskIndexWithFiles, PopulationLoadMode};

    #[test]
    fn witness_population_load_mode_and_index_types() {
        let current = PopulationLoadMode::Current {
            input_fingerprint: "input".to_string(),
            generation_fingerprint: "generation".to_string(),
            selection_context_fingerprint: "context".to_string(),
        };
        let reusable = PopulationLoadMode::ReusablePrior {
            selection_context_fingerprint: "context".to_string(),
        };
        assert!(matches!(current, PopulationLoadMode::Current { .. }));
        assert!(matches!(reusable, PopulationLoadMode::ReusablePrior { .. }));

        let index = OnDiskIndexWithFiles {
            schema_version: INDEX_SCHEMA_VERSION.to_string(),
            source_root: "root".to_string(),
            generation_fingerprint: "generation".to_string(),
            entries_fingerprint: "entries".to_string(),
            files: BTreeMap::new(),
        };
        assert_eq!(index.source_root, "root");
    }
}
