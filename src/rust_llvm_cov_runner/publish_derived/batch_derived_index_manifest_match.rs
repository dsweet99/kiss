use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::rust_llvm_cov_runner::plan::batch_fingerprint::RustCoverageBatchIdentity;

use super::{RustPopulationState, current_test_binaries_match, read_population_manifest};

pub fn current_population_manifest_matches_identity(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> Option<bool> {
    let manifest = read_population_manifest(cache_root)?;
    Some(
        manifest.input_fingerprint == identity.input_digest
            && manifest.generation_fingerprint == identity.generation_fingerprint
            && manifest.selection_context_fingerprint == identity.selection_context_fingerprint
            && manifest.ordinary_source_digests == identity.ordinary_source_digests,
    )
}

pub fn current_population_manifest_matches_universe(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
    selectors: &[String],
) -> Option<bool> {
    if !current_population_manifest_matches_identity(cache_root, identity)? {
        return Some(false);
    }
    let manifest = read_population_manifest(cache_root)?;
    let mut expected = selectors.to_vec();
    expected.sort();
    expected.dedup();
    Some(manifest.selectors == expected)
}

pub fn current_population_manifest_state(
    cache_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> Option<RustPopulationState> {
    if !current_population_manifest_matches_identity(cache_root, identity)? {
        return None;
    }
    let manifest = read_population_manifest(cache_root)?;
    Some(RustPopulationState {
        input_fingerprint: manifest.input_fingerprint,
        generation_fingerprint: manifest.generation_fingerprint,
        selection_context_fingerprint: manifest.selection_context_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index: BTreeMap::new(),
        ordinary_source_digests: manifest.ordinary_source_digests,
        test_binaries: manifest.test_binaries,
    })
}

pub fn current_population_manifest_test_binaries_match(
    cache_root: &Path,
    source_root: &Path,
    identity: &RustCoverageBatchIdentity,
) -> Option<bool> {
    let manifest = read_population_manifest(cache_root)?;
    if manifest.input_fingerprint != identity.input_digest
        || manifest.generation_fingerprint != identity.generation_fingerprint
        || manifest.selection_context_fingerprint != identity.selection_context_fingerprint
    {
        return Some(false);
    }
    let population = RustPopulationState {
        input_fingerprint: manifest.input_fingerprint,
        generation_fingerprint: manifest.generation_fingerprint,
        selection_context_fingerprint: manifest.selection_context_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index: BTreeMap::new(),
        ordinary_source_digests: manifest.ordinary_source_digests,
        test_binaries: manifest.test_binaries,
    };
    Some(current_test_binaries_match(source_root, &population))
}

pub(crate) fn read_population_generation(cache_root: &Path) -> Option<String> {
    let bytes = fs::read(cache_root.join("population.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("generation_fingerprint")?
        .as_str()
        .map(str::to_string)
}
