use super::*;

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
    let mut metrics = empty_metrics();
    metrics.rust_population.summary.rust_build_invocations = 1;
    metrics.rust_final.summary.rust_build_invocations = 2;
    metrics.rust_population.summary.rust_test_instances = 3;
    metrics.rust_final.summary.rust_test_instances = 4;
    metrics.rust_population.summary.rust_export_jobs = 5;
    metrics.rust_final.summary.rust_export_jobs = 6;
    metrics
        .rust_population
        .summary
        .rust_max_active_test_instances = 7;
    metrics.rust_final.summary.rust_max_active_test_instances = 8;
    metrics.rust_population.summary.rust_max_active_exports = 9;
    metrics.rust_final.summary.rust_max_active_exports = 10;

    assert_eq!(rust_build_invocations(&metrics), 3);
    assert_eq!(rust_test_instances(&metrics), 7);
    assert_eq!(rust_export_jobs(&metrics), 11);
    assert_eq!(rust_max_active_test_instances(&metrics), 8);
    assert_eq!(rust_max_active_exports(&metrics), 10);
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

fn empty_metrics() -> LocalRubricMetrics {
    LocalRubricMetrics {
        plan_duration: Duration::ZERO,
        total_duration: Duration::ZERO,
        selected_python: 0,
        python_population_required: false,
        python_population_selectors: 0,
        selected_rust_initial: 0,
        rust_source_paths: 0,
        rust_population_required: false,
        rust_population_selectors: 0,
        rust_final_selectors: 0,
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
