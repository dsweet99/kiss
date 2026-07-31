//! Unit tests for reverse-index telemetry counters and metric keys.

use crate::batch_entry_state::publish_next_entry_state;
use crate::batch_fingerprint::entry_fingerprint;
use crate::batch_reverse_build::BuiltReverseIndex;
use crate::batch_reverse_publish::{
    prune_unreferenced_snapshots, snapshot_path, write_reverse_snapshot,
};
use crate::batch_reverse_query_metrics::ReverseUnavailableReason;
use crate::batch_reverse_test_support::{publish_bound_reverse, write_passed_entry};
use crate::batch_result::RustCoverageBatchCounters;
use crate::rust_cov_cache::store_rust_cov_cache_entry;
use crate::test_support::{derived_fixture_request, witness_batch_tools};
use crate::{
    publish_derived_state, query_reverse_line_index, reset_reverse_query_counters_for_test,
    snapshot_reverse_query_counters, RustCovCacheEntry, RustCovCacheStatus, RustLineCoverage,
    RustLlvmCovOutcome,
};
use rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use tempfile::tempdir;

static METRICS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn with_isolated_reverse_metrics<R>(f: impl FnOnce() -> R) -> R {
    let _guard = METRICS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_reverse_query_counters_for_test();
    f()
}

fn seed(cache: &std::path::Path, source: &std::path::Path) {
    fs::create_dir_all(source.join("src")).unwrap();
    fs::write(source.join("src/lib.rs"), "fn a() {}\n").unwrap();
    write_passed_entry(
        cache,
        "gen1",
        "test_a",
        BTreeMap::from([(
            source.join("src/lib.rs").to_string_lossy().into_owned(),
            BTreeSet::from([1_u32]),
        )]),
    );
}

fn wanted() -> BTreeMap<String, BTreeSet<u32>> {
    BTreeMap::from([("src/lib.rs".into(), BTreeSet::from([1_u32]))])
}

#[test]
fn reverse_hit_increments_query_hit_counter() {
    with_isolated_reverse_metrics(|| {
        let tmp = tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let source = tmp.path().join("src");
        seed(&cache, &source);
        publish_bound_reverse(&cache, &source, "gen1", "fp");
        assert!(query_reverse_line_index(&cache, "gen1", &wanted()).is_some());
        let snap = snapshot_reverse_query_counters();
        assert_eq!(snap.hits, 1);
        assert_eq!(snap.unavailable.total(), 0);
        let mut batch = RustCoverageBatchCounters::default();
        batch.incorporate_process_reverse_query_counters();
        assert_eq!(batch.reverse_query_hits, 1);
        assert_eq!(batch.reverse_unavailable.total(), 0);
        // Second incorporate must not double-count the same process watermark.
        batch.incorporate_process_reverse_query_counters();
        assert_eq!(batch.reverse_query_hits, 1);
    });
}

#[test]
fn forced_schema_unavailable_increments_typed_reason() {
    with_isolated_reverse_metrics(|| {
        let tmp = tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let source = tmp.path().join("src");
        seed(&cache, &source);
        publish_bound_reverse(&cache, &source, "gen1", "fp");
        let mut bad: serde_json::Value =
            serde_json::from_slice(&fs::read(cache.join("population.json")).unwrap()).unwrap();
        bad["reverse_line_index"]["schema_version"] = serde_json::json!("wrong");
        fs::write(
            cache.join("population.json"),
            serde_json::to_vec_pretty(&bad).unwrap(),
        )
        .unwrap();
        assert!(query_reverse_line_index(&cache, "gen1", &wanted()).is_none());
        let snap = snapshot_reverse_query_counters();
        assert_eq!(snap.hits, 0);
        assert_eq!(snap.unavailable.get(ReverseUnavailableReason::Schema), 1);
        assert_eq!(snap.unavailable.total(), 1);
        let mut batch = RustCoverageBatchCounters::default();
        batch.incorporate_process_reverse_query_counters();
        assert_eq!(batch.reverse_query_hits, 0);
        assert_eq!(
            batch
                .reverse_unavailable
                .get(ReverseUnavailableReason::Schema),
            1
        );
    });
}

#[test]
fn publish_derived_state_sets_reverse_published_counter() {
    let repo = tempdir().unwrap();
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(repo.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    let req = derived_fixture_request(repo.path());
    let tools = witness_batch_tools();
    let identity = crate::batch_fingerprint::batch_identity(&req, &tools).unwrap();
    let fingerprint = entry_fingerprint(&identity.input_digest, &req, &tools, "alpha");
    let mut coverage = BTreeMap::new();
    coverage.insert("src/lib.rs".to_string(), BTreeSet::from([1]));
    let entry = RustCovCacheEntry::from_outcome(
        &RustLlvmCovOutcome {
            selector: "alpha".to_string(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: RustLineCoverage { files: coverage },
            test_binary_ids: vec!["test-bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        &identity.generation_fingerprint,
    );
    store_rust_cov_cache_entry(&req.cache_root, &fingerprint, &entry).unwrap();
    let counters =
        publish_derived_state(&req, &tools, &identity, &["alpha".to_string()], true).unwrap();
    assert!(counters.reverse_published);
    let batch = RustCoverageBatchCounters {
        reverse_published: counters.reverse_published,
        reverse_snapshots_reclaimed: counters.reverse_snapshots_reclaimed,
        ..Default::default()
    };
    assert!(batch.reverse_published);
    assert_eq!(batch.reverse_snapshots_reclaimed, counters.reverse_snapshots_reclaimed);
}

#[test]
fn prune_orphan_snapshots_reports_reclaimed_count() {
    let tmp = tempdir().unwrap();
    let cache = tmp.path();
    let built = BuiltReverseIndex {
        selectors: vec!["a".into()],
        files: BTreeMap::new(),
    };
    let r1 = publish_next_entry_state(cache, "gen", "fp1").unwrap();
    let s1 = write_reverse_snapshot(cache, "gen", "fp1", r1, &built).unwrap();
    let r2 = publish_next_entry_state(cache, "gen", "fp2").unwrap();
    let s2 = write_reverse_snapshot(cache, "gen", "fp2", r2, &built).unwrap();
    let r3 = publish_next_entry_state(cache, "gen", "fp3").unwrap();
    let s3 = write_reverse_snapshot(cache, "gen", "fp3", r3, &built).unwrap();
    let removed =
        prune_unreferenced_snapshots(cache, &s3.snapshot_id, Some(&s2.snapshot_id)).unwrap();
    assert!(removed >= 1);
    assert!(!snapshot_path(cache, &s1.snapshot_id).exists());
    let batch = RustCoverageBatchCounters {
        reverse_snapshots_reclaimed: removed,
        ..Default::default()
    };
    assert!(batch.reverse_snapshots_reclaimed >= 1);
}
