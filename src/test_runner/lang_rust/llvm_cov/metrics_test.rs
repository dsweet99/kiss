use super::*;
use rust_llvm_cov_runner::{ReverseUnavailableCounts, RustCoverageBatchCounters};

#[test]
fn batch_counters_are_preserved_for_rust_metrics() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let counters = RustCoverageBatchCounters {
        build_invocations: 1,
        test_instances: 7,
        export_jobs: 5,
        cache_hits: 2,
        max_active_test_instances: 3,
        max_active_exports: 4,
        unmatched_selectors: 1,
        max_objects_per_export: 12,
        build_target_baseline_bytes: 345,
        export_phase_ms: 42,
        ..Default::default()
    };
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters,
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result,
        &kiss::GateConfig::default(),
    ).unwrap();

    assert_eq!(summary.rust_build_invocations, 1);
    assert_eq!(summary.rust_test_instances, 7);
    assert_eq!(summary.rust_export_jobs, 5);
    assert_eq!(summary.rust_batch_cache_hits, 2);
    assert_eq!(summary.rust_max_active_test_instances, 3);
    assert_eq!(summary.rust_max_active_exports, 4);
    assert_eq!(summary.rust_unmatched_selectors, 1);
    assert_eq!(summary.rust_max_objects_per_export, 12);
    assert_eq!(summary.rust_build_target_baseline_bytes, 345);
    assert_eq!(summary.phase_rust_export_ms, 42);
}

#[test]
fn finish_preserves_reverse_batch_counters_in_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let counters = RustCoverageBatchCounters {
        reverse_query_hits: 3,
        reverse_unavailable: ReverseUnavailableCounts {
            schema: 1,
            digest: 2,
            ..Default::default()
        },
        reverse_published: true,
        reverse_snapshots_reclaimed: 5,
        ..Default::default()
    };
    let result = RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: None,
        counters,
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result,
        &kiss::GateConfig::default(),
    ).unwrap();

    assert_eq!(summary.rust_reverse_query_hits, 3);
    assert_eq!(summary.rust_reverse_unavailable_schema, 1);
    assert_eq!(summary.rust_reverse_unavailable_digest, 2);
    assert!(summary.rust_reverse_published);
    assert_eq!(summary.rust_reverse_snapshots_reclaimed, 5);
}
