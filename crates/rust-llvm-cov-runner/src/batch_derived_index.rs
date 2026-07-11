use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::batch_derived::{INDEX_SCHEMA_VERSION, POPULATION_SCHEMA_VERSION};
use crate::batch_fingerprint::RustCoverageBatchIdentity;
use crate::rust_cov_cache::generation_entries_fingerprint;

type RustCoverageIndex = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustPopulationState {
    pub generation_fingerprint: String,
    pub entries_fingerprint: String,
    pub selectors: Vec<String>,
    pub line_index: RustCoverageIndex,
}

pub fn load_current_population_state(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: Option<&[String]>,
) -> Option<RustPopulationState> {
    let manifest = read_population_manifest(cache_root)?;
    let index = read_index_with_files(cache_root)?;
    let normalized_source = normalized_source_root(source_root);
    if index.source_root != normalized_source {
        return None;
    }
    if let Some(selectors) = selectors {
        let mut expected = selectors.to_vec();
        expected.sort();
        expected.dedup();
        if manifest.selectors != expected {
            return None;
        }
    }
    let computed_entries_fingerprint =
        generation_entries_fingerprint(cache_root, &identity.generation_fingerprint).ok()?;
    if manifest.schema_version != POPULATION_SCHEMA_VERSION
        || manifest.generation_fingerprint != identity.generation_fingerprint
        || manifest.input_fingerprint != identity.input_digest
        || index.schema_version != INDEX_SCHEMA_VERSION
        || index.generation_fingerprint != identity.generation_fingerprint
        || index.entries_fingerprint != manifest.entries_fingerprint
        || computed_entries_fingerprint != manifest.entries_fingerprint
    {
        return None;
    }
    Some(RustPopulationState {
        generation_fingerprint: identity.generation_fingerprint.clone(),
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index: index.files,
    })
}

pub fn load_current_generation_line_index(
    cache_root: &Path,
    source_root: &Path,
) -> Option<RustCoverageIndex> {
    let manifest = read_population_manifest(cache_root)?;
    let index = read_index_with_files(cache_root)?;
    let normalized_source = normalized_source_root(source_root);
    if index.source_root != normalized_source {
        return None;
    }
    let computed_entries_fingerprint =
        generation_entries_fingerprint(cache_root, &index.generation_fingerprint).ok()?;
    if index.entries_fingerprint != computed_entries_fingerprint {
        return None;
    }
    if manifest.schema_version != POPULATION_SCHEMA_VERSION
        || manifest.generation_fingerprint != index.generation_fingerprint
        || manifest.entries_fingerprint != index.entries_fingerprint
    {
        return None;
    }
    Some(index.files)
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
struct OnDiskIndexWithFiles {
    schema_version: String,
    source_root: String,
    generation_fingerprint: String,
    entries_fingerprint: String,
    files: RustCoverageIndex,
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

#[derive(Deserialize)]
pub(crate) struct PopulationManifestOnDisk {
    pub(crate) schema_version: String,
    pub(crate) generation_fingerprint: String,
    pub(crate) input_fingerprint: String,
    pub(crate) entries_fingerprint: String,
    pub(crate) selectors: Vec<String>,
}

pub(crate) fn read_population_manifest(cache_root: &Path) -> Option<PopulationManifestOnDisk> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
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
