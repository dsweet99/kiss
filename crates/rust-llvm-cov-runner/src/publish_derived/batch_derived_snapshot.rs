use crate::publish_derived::batch_derived_index::{RustPopulationState, RustSnapshotDelta};
use crate::rust_cov_cache::RustCovCacheEntry;
use crate::{CACHE_SCHEMA_VERSION, RustLineCoverage};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub(crate) fn load_manifest_generation_entries(
    cache_root: &Path,
    source_root: &Path,
    population: &RustPopulationState,
) -> Option<BTreeMap<String, BTreeSet<u32>>> {
    let entries_dir = cache_root.join("entries");
    let mut by_selector = BTreeMap::<String, RustLineCoverage>::new();
    for entry in fs::read_dir(entries_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let parsed: RustCovCacheEntry = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        if parsed.schema_version != CACHE_SCHEMA_VERSION
            || parsed.generation_fingerprint != population.generation_fingerprint
        {
            continue;
        }
        if parsed.status != rpytest_runner::TestStatus::Passed {
            return None;
        }
        if by_selector
            .insert(parsed.selector.clone(), parsed.coverage)
            .is_some()
        {
            return None;
        }
    }
    let actual: Vec<_> = by_selector.keys().cloned().collect();
    if actual != population.selectors {
        return None;
    }
    let mut covered_lines = BTreeMap::<String, BTreeSet<u32>>::new();
    for coverage in by_selector.into_values() {
        for (file, lines) in coverage.files {
            let rel = crate::repo_relative_coverage_file(source_root, &file)?;
            covered_lines.entry(rel).or_default().extend(lines);
        }
    }
    Some(covered_lines)
}

pub(crate) fn stable_generation_coverage_identity(
    population: &RustPopulationState,
    covered_lines: &BTreeMap<String, BTreeSet<u32>>,
) -> String {
    let mut h = stable_fnv1a64(0xcbf2_9ce4_8422_2325, CACHE_SCHEMA_VERSION.as_bytes());
    for value in [
        population.input_fingerprint.as_str(),
        population.generation_fingerprint.as_str(),
        population.selection_context_fingerprint.as_str(),
        population.entries_fingerprint.as_str(),
    ] {
        h = stable_fnv1a64(h, value.as_bytes());
        h = stable_fnv1a64(h, &[0]);
    }
    for selector in &population.selectors {
        h = stable_fnv1a64(h, selector.as_bytes());
        h = stable_fnv1a64(h, &[0]);
    }
    for (file, lines) in covered_lines {
        h = stable_fnv1a64(h, file.as_bytes());
        h = stable_fnv1a64(h, &[0]);
        for line in lines {
            h = stable_fnv1a64(h, line.to_le_bytes().as_slice());
        }
        h = stable_fnv1a64(h, &[0]);
    }
    format!("{h:016x}")
}

fn stable_fnv1a64(h: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0100_0000_01b3;
    bytes
        .iter()
        .fold(h, |acc, byte| (acc ^ u64::from(*byte)).wrapping_mul(PRIME))
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

#[cfg(test)]
#[path = "batch_derived_snapshot_test.rs"]
mod tests;
