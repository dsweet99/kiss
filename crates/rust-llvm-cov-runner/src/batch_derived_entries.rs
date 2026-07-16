use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::batch_derived_index::RustPopulationState;
use crate::rust_cov_cache::RustCovCacheEntry;
use crate::{CACHE_SCHEMA_VERSION, RustLineCoverage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustReusableSelectorEntry {
    pub selector: String,
    pub generation_fingerprint: String,
    pub status: rpytest_runner::TestStatus,
    pub coverage: RustLineCoverage,
    pub test_binary_ids: Vec<String>,
}

pub fn load_reusable_prior_selector_entries(
    cache_root: &Path,
    population: &RustPopulationState,
) -> Option<BTreeMap<String, RustReusableSelectorEntry>> {
    let entries_dir = cache_root.join("entries");
    let mut by_selector = BTreeMap::new();
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
        if parsed.test_binary_ids.is_empty()
            || parsed
                .test_binary_ids
                .iter()
                .any(|id| !population.test_binaries.contains_key(id))
        {
            return None;
        }
        let previous = by_selector.insert(
            parsed.selector.clone(),
            RustReusableSelectorEntry {
                selector: parsed.selector,
                generation_fingerprint: parsed.generation_fingerprint,
                status: parsed.status,
                coverage: parsed.coverage,
                test_binary_ids: parsed.test_binary_ids,
            },
        );
        if previous.is_some() {
            return None;
        }
    }
    let actual = by_selector.keys().cloned().collect::<Vec<_>>();
    (actual == population.selectors).then_some(by_selector)
}
