use crate::batch_derived_index::{
    PopulationLoadMode, RustPopulationState, population_current_identity_matches,
    population_selection_context_matches,
};
use crate::batch_derived_index_types::{
    OnDiskIndexWithFiles, PopulationManifestOnDisk, RustCoverageIndex,
};
use crate::batch_fingerprint::RustCoverageBatchIdentity;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(crate) const CHECK_AGGREGATE_ENTRIES_PREFIX: &str = "check-aggregate:";

pub(crate) fn check_aggregate_entries_fingerprint(
    aggregate: &crate::batch_check_aggregate::ValidatedCheckAggregate,
) -> String {
    format!(
        "{CHECK_AGGREGATE_ENTRIES_PREFIX}{}",
        aggregate.integrity_fingerprint
    )
}

/// Fail-safe: a present but unparseable `index.json` forces population rebuild
/// even when check-aggregate can otherwise skip the index body.
///
/// Large indexes are not consulted on this path. Fully parsing a multi-ten-MB
/// `index.json` into a DOM dominates warm `kiss test` latency, so only small
/// stubs get a full parse; large files are sniffed for a leading `{`.
fn index_json_malformed(cache_root: &Path) -> bool {
    let path = cache_root.join("index.json");
    if !path.is_file() {
        return false;
    }
    let Ok(meta) = fs::metadata(&path) else {
        return true;
    };
    const FULL_PARSE_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
    if meta.len() > FULL_PARSE_BYTE_LIMIT {
        return !index_json_prefix_looks_like_object(&path);
    }
    let Ok(bytes) = fs::read(&path) else {
        return true;
    };
    serde_json::from_slice::<serde_json::Value>(&bytes).is_err()
}

fn index_json_prefix_looks_like_object(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 256];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    buf[..n]
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .copied()
        == Some(b'{')
}

pub(crate) fn load_check_aggregate_population_state(
    cache_root: &Path,
    source_root: &Path,
    selectors: Option<&[String]>,
    mode: PopulationLoadMode,
    manifest: PopulationManifestOnDisk,
) -> Option<RustPopulationState> {
    if index_json_malformed(cache_root) {
        return None;
    }
    let selection_context_fingerprint = population_selection_context_matches(&manifest, &mode)?;
    if !population_current_identity_matches(&manifest, &mode) {
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
    let identity = RustCoverageBatchIdentity {
        input_digest: manifest.input_fingerprint.clone(),
        generation_fingerprint: manifest.generation_fingerprint.clone(),
        selection_context_fingerprint: selection_context_fingerprint.clone(),
        ordinary_source_digests: manifest.ordinary_source_digests.clone(),
    };
    let aggregate = match &mode {
        PopulationLoadMode::Current { .. } => {
            crate::batch_check_aggregate::load_current_check_aggregate_snapshot(
                cache_root,
                source_root,
                &identity,
                Some(&manifest.selectors),
            )?
            .aggregate
        }
        PopulationLoadMode::ReusablePrior {
            selection_context_fingerprint,
        } => crate::batch_check_aggregate::load_reusable_prior_check_aggregate(
            cache_root,
            source_root,
            &manifest.selectors,
            selection_context_fingerprint,
        )?,
    };
    if check_aggregate_entries_fingerprint(&aggregate) != manifest.entries_fingerprint {
        return None;
    }
    if aggregate.selectors != manifest.selectors {
        return None;
    }
    // Compact in-memory index: covered-file keys only. Values are unused for
    // check-aggregate selection (any hit selects the full selector universe).
    let line_index = aggregate
        .aggregate_covered_lines
        .keys()
        .map(|file| (file.clone(), BTreeSet::new()))
        .collect();
    Some(RustPopulationState {
        input_fingerprint: manifest.input_fingerprint,
        generation_fingerprint: manifest.generation_fingerprint,
        selection_context_fingerprint,
        entries_fingerprint: manifest.entries_fingerprint,
        selectors: manifest.selectors,
        line_index,
        ordinary_source_digests: manifest.ordinary_source_digests,
        test_binaries: manifest.test_binaries,
    })
}

/// True when the population is backed by check-aggregate (conservative: any
/// covered file selects the full selector universe).
pub fn is_check_aggregate_population(state: &RustPopulationState) -> bool {
    state
        .entries_fingerprint
        .starts_with(CHECK_AGGREGATE_ENTRIES_PREFIX)
}

pub(crate) fn index_matches_check_aggregate(
    cache_root: &Path,
    source_root: &Path,
    manifest: &PopulationManifestOnDisk,
    index: &OnDiskIndexWithFiles,
) -> bool {
    let identity = RustCoverageBatchIdentity {
        input_digest: manifest.input_fingerprint.clone(),
        generation_fingerprint: manifest.generation_fingerprint.clone(),
        selection_context_fingerprint: manifest.selection_context_fingerprint.clone(),
        ordinary_source_digests: manifest.ordinary_source_digests.clone(),
    };
    let Some(snapshot) = crate::batch_check_aggregate::load_current_check_aggregate_snapshot(
        cache_root,
        source_root,
        &identity,
        Some(&manifest.selectors),
    ) else {
        return false;
    };
    manifest.entries_fingerprint == check_aggregate_entries_fingerprint(&snapshot.aggregate)
        && conservative_check_aggregate_index(&snapshot.aggregate) == index.files
}

fn conservative_check_aggregate_index(
    aggregate: &crate::batch_check_aggregate::ValidatedCheckAggregate,
) -> RustCoverageIndex {
    let selectors = aggregate.selectors.iter().cloned().collect::<BTreeSet<_>>();
    aggregate
        .aggregate_covered_lines
        .keys()
        .map(|file| (file.clone(), selectors.clone()))
        .collect()
}
