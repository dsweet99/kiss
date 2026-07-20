use super::super::metrics_rust::{
    phase_rust_export_ms, rust_build_invocations, rust_build_target_baseline_bytes,
    rust_cache_pruned_entries, rust_current_index_generation, rust_derived_repair,
    rust_entry_generation_count, rust_export_jobs, rust_legacy_cleanup_deferred,
    rust_max_active_exports, rust_max_active_test_instances, rust_max_objects_per_export,
    rust_process_residual_count, rust_test_instances, rust_unmatched_selectors,
};
use super::*;
use rust_llvm_cov_runner::RustCoverageBatchCounters;
use std::path::PathBuf;

#[test]
fn phase_metrics_prints_zero_summary() {
    let phase = PhaseMetrics::default();

    assert_eq!(phase.summary.total, 0);
    print_phase_metrics("phase", &phase);
}

#[test]
fn rust_cache_unstored_sums_population_and_final_phases() {
    let mut metrics = empty_metrics();
    metrics.rust_population.summary.cache_unstored = 2;
    metrics.rust_final.summary.cache_unstored = 3;

    assert_eq!(rust_cache_unstored(&metrics), 5);
}

#[test]
fn rust_batch_metric_helpers_merge_population_and_final_phases() {
    let metrics = metrics_with_batch_counters();

    assert_eq!(rust_build_invocations(&metrics), 3);
    assert_eq!(rust_test_instances(&metrics), 7);
    assert_eq!(rust_export_jobs(&metrics), 11);
    assert_eq!(rust_max_active_test_instances(&metrics), 8);
    assert_eq!(rust_max_active_exports(&metrics), 10);
    assert_eq!(rust_unmatched_selectors(&metrics), 3);
    assert_eq!(rust_max_objects_per_export(&metrics), 12);
    assert_eq!(rust_build_target_baseline_bytes(&metrics), 90);
    assert_eq!(phase_rust_export_ms(&metrics), 150);
}

#[test]
fn rust_batch_derived_metric_helpers_merge_population_and_final_phases() {
    let metrics = metrics_with_batch_counters();

    assert!(rust_derived_repair(&metrics));
    assert_eq!(rust_entry_generation_count(&metrics), 3);
    assert_eq!(rust_current_index_generation(&metrics), "gen-b");
    assert_eq!(rust_cache_pruned_entries(&metrics), 9);
    assert_eq!(rust_process_residual_count(&metrics), 13);
    assert!(rust_legacy_cleanup_deferred(&metrics));
}

fn metrics_with_batch_counters() -> LocalRubricMetrics {
    let mut metrics = empty_metrics();
    metrics.rust_population.summary = population_rust_summary();
    metrics.rust_final.summary = final_rust_summary();
    metrics
}

#[test]
fn rust_batch_counters_record_process_and_cleanup_metrics() {
    let mut summary = SelectorExecutionSummary::default();
    let counters = RustCoverageBatchCounters {
        process_residual_count: 2,
        legacy_cleanup_deferred: true,
        ..RustCoverageBatchCounters::default()
    };

    summary.record_rust_batch_counters(&counters);

    assert_eq!(summary.rust_process_residual_count, 2);
    assert!(summary.rust_legacy_cleanup_deferred);
}

fn population_rust_summary() -> SelectorExecutionSummary {
    SelectorExecutionSummary {
        rust_build_invocations: 1,
        rust_test_instances: 3,
        rust_export_jobs: 5,
        rust_max_active_test_instances: 7,
        rust_max_active_exports: 9,
        rust_unmatched_selectors: 1,
        rust_max_objects_per_export: 11,
        rust_build_target_baseline_bytes: 80,
        phase_rust_export_ms: 100,
        rust_derived_repair: true,
        rust_entry_generation_count: 2,
        rust_current_index_generation: "gen-a".to_string(),
        rust_cache_pruned_entries: 4,
        rust_process_residual_count: 6,
        ..SelectorExecutionSummary::default()
    }
}

fn final_rust_summary() -> SelectorExecutionSummary {
    SelectorExecutionSummary {
        rust_build_invocations: 2,
        rust_test_instances: 4,
        rust_export_jobs: 6,
        rust_max_active_test_instances: 8,
        rust_max_active_exports: 10,
        rust_unmatched_selectors: 2,
        rust_max_objects_per_export: 12,
        rust_build_target_baseline_bytes: 90,
        phase_rust_export_ms: 50,
        rust_entry_generation_count: 3,
        rust_current_index_generation: "gen-b".to_string(),
        rust_cache_pruned_entries: 5,
        rust_process_residual_count: 7,
        rust_legacy_cleanup_deferred: true,
        ..SelectorExecutionSummary::default()
    }
}

#[test]
fn metrics_print_helpers_accept_empty_metrics() {
    let metrics = empty_metrics();

    print_oracle_metrics();
    print_selection_metrics(&metrics);
    print_timing_metrics(&metrics);
    print_cache_metrics(&metrics);
    metrics.print();
}

#[test]
fn cache_shape_records_entry_and_build_target_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let rust_cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    fs::create_dir_all(rust_cache.join("entries")).unwrap();
    fs::create_dir_all(rust_cache.join("build").join("target")).unwrap();
    fs::write(rust_cache.join("entries").join("case.json"), b"abc").unwrap();
    fs::write(
        rust_cache.join("build").join("target").join("artifact"),
        b"defg",
    )
    .unwrap();
    let mut metrics = empty_metrics();

    metrics.capture_cache_shape(tmp.path());

    assert_eq!(metrics.rust_entry_cache_bytes, 3);
    assert_eq!(metrics.rust_build_target_bytes, 4);
}

#[test]
fn cache_shape_records_external_tmp_residuals() {
    let tmp = tempfile::tempdir().unwrap();
    let rust_cache = tmp.path().join(".kiss").join("rust_llvm_cov_cache");
    fs::create_dir_all(&rust_cache).unwrap();
    let external = rust_cov_cache_tmp_parent(&rust_cache);
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("residual.profraw"), b"abcde").unwrap();
    let mut metrics = empty_metrics();

    metrics.capture_cache_shape(tmp.path());

    assert_eq!(metrics.rust_external_tmp_residual_bytes, 5);
    assert_eq!(metrics.rust_external_tmp_residual_count, 1);
    assert!(!metrics.rust_external_tmp_metric_error);
    assert_eq!(metrics.rust_transient_residual_count, 1);
    fs::remove_dir_all(external).unwrap();
}

fn empty_metrics() -> LocalRubricMetrics {
    LocalRubricMetrics {
        plan_duration: Duration::ZERO,
        total_duration: Duration::ZERO,
        selected_python: 0,
        python_population_required: false,
        python_population_selectors: 0,
        selected_rust_initial: 0,
        rust_source_paths: 0,
        rust_vcs_source_paths: 0,
        rust_snapshot_delta_modified: 0,
        rust_snapshot_delta_structural: false,
        rust_population_required: false,
        rust_population_selectors: 0,
        rust_final_selectors: 0,
        rust_selection_basis: Default::default(),
        coverage_decision_engine_used: true,
        python: PhaseMetrics::default(),
        python_index_rebuild_duration: Duration::ZERO,
        rust_population: PhaseMetrics::default(),
        rust_index_rebuild_duration: Duration::ZERO,
        rust_final: PhaseMetrics::default(),
        kiss_cache_residual_bytes: 0,
        rust_cache_residual_bytes: 0,
        rust_entry_cache_bytes: 0,
        rust_build_target_bytes: 0,
        rust_build_target_baseline_bytes: 0,
        raw_artifact_count: 0,
        rust_build_target_count: 0,
        rust_transient_residual_count: 0,
        rust_external_tmp_residual_bytes: 0,
        rust_external_tmp_residual_count: 0,
        rust_external_tmp_metric_error: false,
        rust_concurrency_budget: 1,
        exit_code: 0,
    }
}

#[test]
fn local_rubric_metrics_carry_reusable_prior_selection_basis() {
    use crate::test_runner::coverage_decision::RustSelectionBasis;
    use crate::test_runner::{PlannedSelectors, SelectorRunOptions};

    let planned = PlannedSelectors {
        repo_root: PathBuf::from("/repo"),
        py_sel: Vec::new(),
        rs_sel: vec!["tests::gets_value".to_string()],
        python_population_required: false,
        rust_population_required: false,
        rust_source_paths: vec![PathBuf::from("src/lib.rs")],
        rust_vcs_source_paths: 168,
        rust_snapshot_delta_modified: 1,
        rust_snapshot_delta_structural: false,
        python_prior_failure_selectors: Vec::new(),
        rust_prior_failure_selectors: Vec::new(),
        coverage_decision_engine_used: true,
        rust_selection_basis: RustSelectionBasis::ReusablePrior,
        ignore: Vec::new(),
    };
    let options = SelectorRunOptions {
        dry_run: true,
        jobs: 1,
        plan_duration: Duration::ZERO,
        force_rerun: false,
        metrics: false,
        extra: &[],
    };
    let metrics = LocalRubricMetrics::new(
        &planned,
        &options,
        0,
        false,
        0,
        planned.rs_sel.len(),
        planned.rust_selection_basis,
    );
    assert_eq!(
        metrics.rust_selection_basis,
        RustSelectionBasis::ReusablePrior
    );
    assert_eq!(metrics.rust_vcs_source_paths, 168);
    assert_eq!(metrics.rust_snapshot_delta_modified, 1);
    assert!(!metrics.rust_snapshot_delta_structural);
    assert!(!metrics.rust_population_required);
}

#[cfg(unix)]
#[test]
fn path_size_and_count_returns_error_for_unreadable_directory() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let blocked = tmp.path().join("blocked");
    fs::create_dir(&blocked).unwrap();
    let original_permissions = fs::metadata(&blocked).unwrap().permissions();
    let mut blocked_permissions = original_permissions.clone();
    blocked_permissions.set_mode(0o000);
    fs::set_permissions(&blocked, blocked_permissions).unwrap();

    let result = path_size_and_count(&blocked);

    fs::set_permissions(&blocked, original_permissions).unwrap();
    assert!(result.is_err());
}
