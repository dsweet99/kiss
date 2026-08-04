use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::publish_derived::batch_derived_index::RustPopulationState;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_cov_cache::RustCovCacheEntry;
    use crate::{RustCovCacheStatus, RustLlvmCovOutcome, RustTestBinaryIdentity};
    use rpytest_runner::TestStatus;
    use std::time::Duration;

    fn population() -> RustPopulationState {
        RustPopulationState {
            input_fingerprint: "input".to_string(),
            generation_fingerprint: "generation".to_string(),
            selection_context_fingerprint: "selection".to_string(),
            entries_fingerprint: "entries".to_string(),
            selectors: vec!["test_a".to_string(), "test_b".to_string()],
            line_index: BTreeMap::new(),
            ordinary_source_digests: BTreeMap::new(),
            test_binaries: BTreeMap::from([(
                "bin".to_string(),
                RustTestBinaryIdentity {
                    id: "bin".to_string(),
                    executable: "/tmp/bin".to_string(),
                    digest: "digest".to_string(),
                },
            )]),
        }
    }

    fn entry(selector: &str, generation: &str, binary_ids: Vec<String>) -> RustCovCacheEntry {
        RustCovCacheEntry::from_outcome(
            &RustLlvmCovOutcome {
                selector: selector.to_string(),
                status: TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::from([(
                        "src/lib.rs".to_string(),
                        [1_u32].into_iter().collect(),
                    )]),
                },
                test_binary_ids: binary_ids,
                cache_status: RustCovCacheStatus::Hit,
                stdout: None,
                stderr: None,
            },
            generation,
        )
    }

    fn write_entry(cache_root: &Path, name: &str, entry: &RustCovCacheEntry) {
        let entries = cache_root.join("entries");
        fs::create_dir_all(&entries).unwrap();
        fs::write(
            entries.join(name),
            serde_json::to_vec(entry).expect("entry json"),
        )
        .unwrap();
    }

    #[test]
    fn loads_entries_when_generation_and_selector_population_match() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(
            tmp.path(),
            "a.json",
            &entry("test_a", "generation", vec!["bin".to_string()]),
        );
        write_entry(
            tmp.path(),
            "b.json",
            &entry("test_b", "generation", vec!["bin".to_string()]),
        );

        let entries = load_reusable_prior_selector_entries(tmp.path(), &population()).unwrap();

        assert_eq!(
            entries.keys().cloned().collect::<Vec<_>>(),
            ["test_a", "test_b"]
        );
        assert_eq!(entries["test_a"].test_binary_ids, vec!["bin".to_string()]);
    }

    #[test]
    fn rejects_missing_or_unknown_binary_entries() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(
            tmp.path(),
            "a.json",
            &entry("test_a", "generation", vec!["unknown".to_string()]),
        );
        write_entry(
            tmp.path(),
            "b.json",
            &entry("test_b", "generation", vec!["bin".to_string()]),
        );

        assert!(load_reusable_prior_selector_entries(tmp.path(), &population()).is_none());
    }

    #[test]
    fn ignores_stale_generation_entries_but_requires_complete_population() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(
            tmp.path(),
            "a.json",
            &entry("test_a", "old-generation", vec!["bin".to_string()]),
        );
        write_entry(
            tmp.path(),
            "b.json",
            &entry("test_b", "generation", vec!["bin".to_string()]),
        );

        assert!(load_reusable_prior_selector_entries(tmp.path(), &population()).is_none());
    }
}
