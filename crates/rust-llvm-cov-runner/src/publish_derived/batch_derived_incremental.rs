use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::publish_derived::batch_derived::{DerivedPublishCounters, publish_derived_state_with_binaries};
use crate::plan::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError, RustTestBinaryIdentity};

pub struct IncrementalPublishPlan<'a> {
    pub prior_generation: &'a str,
    pub selectors: &'a [String],
    pub retained_selectors: &'a [String],
    pub expected_selector_binaries: &'a BTreeMap<String, Vec<String>>,
    pub test_binaries: &'a [RustTestBinaryIdentity],
}

/// Re-store prior-generation selector entries under the current identity.
///
/// Preserves measured `duration` (and coverage bindings) while updating
/// `generation_fingerprint` and the entry fingerprint path so current-population
/// timing loads remain complete after CheckAggregate / SelectorEntries repair.
pub fn rekey_selector_entries_to_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    current: &RustCoverageBatchIdentity,
    prior_generation: &str,
    selectors: &[String],
) -> Result<(), RustLlvmCovError> {
    // One directory scan: per-selector full-directory walks are O(selectors×entries)
    // and hang `kiss cov` on large caches (~10k entries × ~1.5k selectors).
    let by_selector = index_generation_selector_entries(&req.cache_root, prior_generation);
    for selector in selectors {
        let mut entry = by_selector.get(selector).cloned().ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing retained Rust coverage entry for selector `{selector}`"
            ))
        })?;
        entry.generation_fingerprint = current.generation_fingerprint.clone();
        let fingerprint = crate::plan::batch_fingerprint::entry_fingerprint(
            &current.input_digest,
            req,
            tools,
            selector,
        );
        store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry)
            .map_err(RustLlvmCovError::Io)?;
    }
    Ok(())
}

pub fn publish_incremental_derived_state(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    current: &RustCoverageBatchIdentity,
    plan: IncrementalPublishPlan<'_>,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    rekey_selector_entries_to_identity(
        req,
        tools,
        current,
        plan.prior_generation,
        plan.retained_selectors,
    )?;
    validate_successor_entries(
        &req.cache_root,
        &current.generation_fingerprint,
        plan.selectors,
        plan.expected_selector_binaries,
    )?;
    publish_derived_state_with_binaries(
        req,
        tools,
        current,
        plan.selectors,
        plan.test_binaries,
        true,
    )
}

fn validate_successor_entries(
    cache_root: &Path,
    generation: &str,
    selectors: &[String],
    expected_selector_binaries: &BTreeMap<String, Vec<String>>,
) -> Result<(), RustLlvmCovError> {
    let by_selector = index_generation_selector_entries(cache_root, generation);
    for selector in selectors {
        let expected = expected_selector_binaries.get(selector).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing executable metadata for Rust selector `{selector}`"
            ))
        })?;
        let entry = by_selector.get(selector).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing successor Rust coverage entry for selector `{selector}`"
            ))
        })?;
        if entry.status != rpytest_runner::TestStatus::Passed {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "successor Rust coverage entry for selector `{selector}` did not pass"
            )));
        }
        if &entry.test_binary_ids != expected {
            return Err(RustLlvmCovError::InvalidRequest(format!(
                "successor Rust coverage entry for selector `{selector}` has executable binding {:?}, expected {:?}",
                entry.test_binary_ids, expected
            )));
        }
    }
    Ok(())
}

/// Single pass over `entries/` for one generation → selector map (first wins).
fn index_generation_selector_entries(
    cache_root: &Path,
    generation: &str,
) -> BTreeMap<String, RustCovCacheEntry> {
    let mut by_selector = BTreeMap::new();
    let entries_dir = cache_root.join("entries");
    let Ok(entries) = fs::read_dir(entries_dir) else {
        return by_selector;
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
        if parsed.schema_version == CACHE_SCHEMA_VERSION
            && parsed.generation_fingerprint == generation
        {
            by_selector.entry(parsed.selector.clone()).or_insert(parsed);
        }
    }
    by_selector
}

#[cfg(test)]
fn load_generation_selector_entry(
    cache_root: &Path,
    generation: &str,
    selector: &str,
) -> Option<RustCovCacheEntry> {
    index_generation_selector_entries(cache_root, generation)
        .get(selector)
        .cloned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use rpytest_runner::TestStatus;

    use crate::plan::batch_fingerprint::{batch_identity, entry_fingerprint};
    use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
    use crate::test_support::{derived_fixture_request, witness_batch_tools};
    use crate::{RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};

    fn store_entry_with_status(
        req: &crate::RustCoverageBatchRequest,
        tools: &crate::RustCoverageToolIdentity,
        generation: &str,
        selector: &str,
        test_binary_ids: Vec<String>,
        status: TestStatus,
    ) {
        let identity = batch_identity(req, tools).unwrap();
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
        let entry = RustCovCacheEntry::from_outcome(
            &RustLlvmCovOutcome {
                selector: selector.to_string(),
                status,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids,
                cache_status: RustCovCacheStatus::MissStored,
                stdout: None,
                stderr: None,
            },
            generation,
        );
        store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
    }

    fn store_entry(
        req: &crate::RustCoverageBatchRequest,
        tools: &crate::RustCoverageToolIdentity,
        generation: &str,
        selector: &str,
        test_binary_ids: Vec<String>,
    ) {
        store_entry_with_status(
            req,
            tools,
            generation,
            selector,
            test_binary_ids,
            TestStatus::Passed,
        );
    }

    fn test_binary() -> crate::RustTestBinaryIdentity {
        crate::RustTestBinaryIdentity {
            id: "test-bin".to_string(),
            executable: "/tmp/test-bin".to_string(),
            digest: "0000000000000000".to_string(),
        }
    }

    #[test]
    fn successor_validation_rejects_changed_selector_binary_binding() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let req = derived_fixture_request(repo.path());
        let tools = witness_batch_tools();
        let identity = batch_identity(&req, &tools).unwrap();
        store_entry(
            &req,
            &tools,
            &identity.generation_fingerprint,
            "alpha",
            vec!["wrong-bin".to_string()],
        );

        let expected = BTreeMap::from([("alpha".to_string(), vec!["test-bin".to_string()])]);

        let err = super::validate_successor_entries(
            &req.cache_root,
            &identity.generation_fingerprint,
            &["alpha".to_string()],
            &expected,
        )
        .unwrap_err();
        assert!(format!("{err:?}").contains("executable binding"));
    }

    #[test]
    fn successor_validation_rejects_missing_metadata_and_failed_entries() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let req = derived_fixture_request(repo.path());
        let tools = witness_batch_tools();
        let identity = batch_identity(&req, &tools).unwrap();

        let missing_metadata = super::validate_successor_entries(
            &req.cache_root,
            &identity.generation_fingerprint,
            &["alpha".to_string()],
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(format!("{missing_metadata:?}").contains("missing executable metadata"));

        store_entry_with_status(
            &req,
            &tools,
            &identity.generation_fingerprint,
            "alpha",
            vec!["test-bin".to_string()],
            TestStatus::Failed,
        );
        let expected = BTreeMap::from([("alpha".to_string(), vec!["test-bin".to_string()])]);
        let failed = super::validate_successor_entries(
            &req.cache_root,
            &identity.generation_fingerprint,
            &["alpha".to_string()],
            &expected,
        )
        .unwrap_err();
        assert!(format!("{failed:?}").contains("did not pass"));
    }

    #[test]
    fn incremental_publish_rewrites_retained_entry_to_current_generation() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let req = derived_fixture_request(repo.path());
        let tools = witness_batch_tools();
        let current = batch_identity(&req, &tools).unwrap();
        let prior_generation = "prior-generation";
        store_entry(
            &req,
            &tools,
            prior_generation,
            "alpha",
            vec!["test-bin".to_string()],
        );
        let expected = BTreeMap::from([("alpha".to_string(), vec!["test-bin".to_string()])]);
        let selectors = vec!["alpha".to_string()];
        let counters = super::publish_incremental_derived_state(
            &req,
            &tools,
            &current,
            super::IncrementalPublishPlan {
                prior_generation,
                selectors: &selectors,
                retained_selectors: &selectors,
                expected_selector_binaries: &expected,
                test_binaries: &[test_binary()],
            },
        )
        .unwrap();

        assert!(counters.entry_generation_count > 0);
        let entry = super::load_generation_selector_entry(
            &req.cache_root,
            &current.generation_fingerprint,
            "alpha",
        )
        .expect("current-generation retained entry");
        assert_eq!(entry.generation_fingerprint, current.generation_fingerprint);
        assert_eq!(entry.test_binary_ids, vec!["test-bin".to_string()]);
    }

    #[test]
    fn rekey_and_validate_scale_with_many_decoy_entries() {
        // Regression: per-selector full-directory scans made kiss cov hang on large caches.
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(repo.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
        let req = derived_fixture_request(repo.path());
        let tools = witness_batch_tools();
        let current = batch_identity(&req, &tools).unwrap();
        let prior_generation = "prior-generation";
        let entries = req.cache_root.join("entries");
        std::fs::create_dir_all(&entries).unwrap();
        for i in 0..2_000 {
            let decoy = RustCovCacheEntry::from_outcome(
                &RustLlvmCovOutcome {
                    selector: format!("decoy_{i}"),
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    duration: Duration::from_millis(1),
                    coverage: RustLineCoverage {
                        files: BTreeMap::new(),
                    },
                    test_binary_ids: vec!["test-bin".to_string()],
                    cache_status: RustCovCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                },
                prior_generation,
            );
            store_rust_cov_cache_entry(&req.cache_root, &format!("decoy{i:04x}"), &decoy).unwrap();
        }
        store_entry(
            &req,
            &tools,
            prior_generation,
            "alpha",
            vec!["test-bin".to_string()],
        );
        let expected = BTreeMap::from([("alpha".to_string(), vec!["test-bin".to_string()])]);
        let selectors = vec!["alpha".to_string()];
        let started = std::time::Instant::now();
        super::publish_incremental_derived_state(
            &req,
            &tools,
            &current,
            super::IncrementalPublishPlan {
                prior_generation,
                selectors: &selectors,
                retained_selectors: &selectors,
                expected_selector_binaries: &expected,
                test_binaries: &[test_binary()],
            },
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "indexed lookup must stay fast with thousands of decoy entries"
        );
    }
}
