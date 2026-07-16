use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::batch_derived::{DerivedPublishCounters, publish_derived_state_with_binaries};
use crate::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use crate::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
use crate::{CACHE_SCHEMA_VERSION, RustLlvmCovError, RustTestBinaryIdentity};

pub struct IncrementalPublishPlan<'a> {
    pub prior_generation: &'a str,
    pub selectors: &'a [String],
    pub retained_selectors: &'a [String],
    pub expected_selector_binaries: &'a BTreeMap<String, Vec<String>>,
    pub test_binaries: &'a [RustTestBinaryIdentity],
}

pub fn publish_incremental_derived_state(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    current: &RustCoverageBatchIdentity,
    plan: IncrementalPublishPlan<'_>,
) -> Result<DerivedPublishCounters, RustLlvmCovError> {
    for selector in plan.retained_selectors {
        let mut entry =
            load_generation_selector_entry(&req.cache_root, plan.prior_generation, selector)
                .ok_or_else(|| {
                    RustLlvmCovError::InvalidRequest(format!(
                        "missing retained Rust coverage entry for selector `{selector}`"
                    ))
                })?;
        entry.generation_fingerprint = current.generation_fingerprint.clone();
        let fingerprint = crate::batch_fingerprint::entry_fingerprint(
            &current.input_digest,
            req,
            tools,
            selector,
        );
        store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry)
            .map_err(RustLlvmCovError::Io)?;
    }
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
    for selector in selectors {
        let expected = expected_selector_binaries.get(selector).ok_or_else(|| {
            RustLlvmCovError::InvalidRequest(format!(
                "missing executable metadata for Rust selector `{selector}`"
            ))
        })?;
        let entry =
            load_generation_selector_entry(cache_root, generation, selector).ok_or_else(|| {
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

fn load_generation_selector_entry(
    cache_root: &Path,
    generation: &str,
    selector: &str,
) -> Option<RustCovCacheEntry> {
    let entries_dir = cache_root.join("entries");
    for entry in fs::read_dir(entries_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let parsed: RustCovCacheEntry = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        if parsed.schema_version == CACHE_SCHEMA_VERSION
            && parsed.generation_fingerprint == generation
            && parsed.selector == selector
        {
            return Some(parsed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use rpytest_runner::TestStatus;

    use crate::batch_fingerprint::{batch_identity, entry_fingerprint};
    use crate::rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry};
    use crate::test_support::{derived_fixture_request, witness_batch_tools};
    use crate::{RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};

    fn store_entry(
        req: &crate::RustCoverageBatchRequest,
        tools: &crate::RustCoverageToolIdentity,
        generation: &str,
        selector: &str,
        test_binary_ids: Vec<String>,
    ) {
        let identity = batch_identity(req, tools).unwrap();
        let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
        let entry = RustCovCacheEntry::from_outcome(
            &RustLlvmCovOutcome {
                selector: selector.to_string(),
                status: TestStatus::Passed,
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
}
