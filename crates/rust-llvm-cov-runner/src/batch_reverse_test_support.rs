//! Shared helpers for reverse-index unit and process-race tests.

use crate::batch_entry_state::publish_next_entry_state;
use crate::batch_lock::lock_batch;
use crate::batch_reverse_publish::publish_reverse_line_index;
use crate::publish_derived_state;
use crate::test_support::witness_batch_tools;
use crate::{RustCovCacheEntry, RustLineCoverage, RustLlvmCovOutcome};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Duration;

pub fn write_population_with_reverse(
    cache: &Path,
    generation: &str,
    entries_fp: &str,
    info: &crate::batch_reverse_build::ReversePublishInfo,
) {
    let payload = serde_json::json!({
        "schema_version": "rust-llvm-cov-population-v6",
        "generation_fingerprint": generation,
        "entries_fingerprint": entries_fp,
        "reverse_line_index": {
            "schema_version": info.schema_version,
            "snapshot_id": info.snapshot_id,
            "meta_digest": info.meta_digest,
            "entry_state_revision": info.entry_state_revision,
        }
    });
    fs::write(
        cache.join("population.json"),
        serde_json::to_vec_pretty(&payload).unwrap(),
    )
    .unwrap();
}

pub fn publish_bound_reverse(
    cache: &Path,
    source: &Path,
    generation: &str,
    entries_fp: &str,
) -> crate::batch_reverse_build::ReversePublishInfo {
    let revision = publish_next_entry_state(cache, generation, entries_fp).unwrap();
    let info =
        publish_reverse_line_index(cache, source, generation, entries_fp, revision).unwrap();
    write_population_with_reverse(cache, generation, entries_fp, &info);
    info
}

pub fn write_passed_entry(
    cache_root: &Path,
    generation: &str,
    selector: &str,
    files: BTreeMap<String, BTreeSet<u32>>,
) {
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: selector.to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files },
            test_binary_ids: vec!["bin".into()],
            cache_status: crate::RustCovCacheStatus::Hit,
            stdout: None,
            stderr: None,
        },
        generation,
    );
    let dir = cache_root.join("entries");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{selector}.json")),
        serde_json::to_vec_pretty(&entry).unwrap(),
    )
    .unwrap();
}

pub fn seed_alpha_beta_reverse(req: &crate::RustCoverageBatchRequest) -> String {
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(req, &tools).unwrap();
    let _guard = lock_batch(&req.cache_root).unwrap();
    publish_derived_state(
        req,
        &tools,
        &identity,
        &["alpha".to_string(), "beta".to_string()],
        true,
    )
    .unwrap();
    serde_json::from_slice::<serde_json::Value>(
        &fs::read(req.cache_root.join("population.json")).unwrap(),
    )
    .unwrap()["reverse_line_index"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string()
}
