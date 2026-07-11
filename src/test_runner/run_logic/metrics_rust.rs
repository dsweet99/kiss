use super::LocalRubricMetrics;

pub(super) fn print_rust_batch_metrics(metrics: &LocalRubricMetrics) {
    println!("rust_build_invocations={}", rust_build_invocations(metrics));
    println!(
        "rust_build_target_count={}",
        metrics.rust_build_target_count
    );
    println!(
        "rust_build_target_bytes={}",
        metrics.rust_build_target_bytes
    );
    println!(
        "rust_build_target_baseline_bytes={}",
        rust_build_target_baseline_bytes(metrics)
    );
    println!("rust_test_instances={}", rust_test_instances(metrics));
    println!("rust_export_jobs={}", rust_export_jobs(metrics));
    println!(
        "rust_max_active_test_instances={}",
        rust_max_active_test_instances(metrics)
    );
    println!(
        "rust_max_active_exports={}",
        rust_max_active_exports(metrics)
    );
    println!(
        "rust_unmatched_selectors={}",
        rust_unmatched_selectors(metrics)
    );
    println!(
        "rust_max_objects_per_export={}",
        rust_max_objects_per_export(metrics)
    );
    println!("phase_rust_export_ms={}", phase_rust_export_ms(metrics));
    println!("rust_derived_repair={}", rust_derived_repair(metrics));
    println!(
        "rust_entry_generation_count={}",
        rust_entry_generation_count(metrics)
    );
    println!(
        "rust_current_index_generation={}",
        rust_current_index_generation(metrics)
    );
    println!(
        "rust_cache_pruned_entries={}",
        rust_cache_pruned_entries(metrics)
    );
    println!(
        "rust_process_residual_count={}",
        rust_process_residual_count(metrics)
    );
    println!(
        "rust_legacy_cleanup_deferred={}",
        rust_legacy_cleanup_deferred(metrics)
    );
}

pub(super) fn rust_cache_unstored(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.cache_unstored + metrics.rust_final.summary.cache_unstored
}

pub(super) fn rust_build_invocations(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_build_invocations
        + metrics.rust_final.summary.rust_build_invocations
}

pub(super) fn rust_test_instances(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_test_instances
        + metrics.rust_final.summary.rust_test_instances
}

pub(super) fn rust_export_jobs(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_export_jobs + metrics.rust_final.summary.rust_export_jobs
}

pub(super) fn rust_max_active_test_instances(metrics: &LocalRubricMetrics) -> usize {
    metrics
        .rust_population
        .summary
        .rust_max_active_test_instances
        .max(metrics.rust_final.summary.rust_max_active_test_instances)
}

pub(super) fn rust_max_active_exports(metrics: &LocalRubricMetrics) -> usize {
    metrics
        .rust_population
        .summary
        .rust_max_active_exports
        .max(metrics.rust_final.summary.rust_max_active_exports)
}

pub(super) fn rust_unmatched_selectors(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_unmatched_selectors
        + metrics.rust_final.summary.rust_unmatched_selectors
}

pub(super) fn rust_max_objects_per_export(metrics: &LocalRubricMetrics) -> usize {
    metrics
        .rust_population
        .summary
        .rust_max_objects_per_export
        .max(metrics.rust_final.summary.rust_max_objects_per_export)
}

pub(super) fn rust_build_target_baseline_bytes(metrics: &LocalRubricMetrics) -> u64 {
    metrics
        .rust_population
        .summary
        .rust_build_target_baseline_bytes
        .max(metrics.rust_final.summary.rust_build_target_baseline_bytes)
        .max(metrics.rust_build_target_baseline_bytes)
}

pub(super) fn phase_rust_export_ms(metrics: &LocalRubricMetrics) -> u128 {
    metrics.rust_population.summary.phase_rust_export_ms
        + metrics.rust_final.summary.phase_rust_export_ms
}

pub(super) fn rust_derived_repair(metrics: &LocalRubricMetrics) -> bool {
    metrics.rust_population.summary.rust_derived_repair
        || metrics.rust_final.summary.rust_derived_repair
}

pub(super) fn rust_entry_generation_count(metrics: &LocalRubricMetrics) -> usize {
    metrics
        .rust_population
        .summary
        .rust_entry_generation_count
        .max(metrics.rust_final.summary.rust_entry_generation_count)
}

pub(super) fn rust_current_index_generation(metrics: &LocalRubricMetrics) -> &str {
    if !metrics
        .rust_final
        .summary
        .rust_current_index_generation
        .is_empty()
    {
        return &metrics.rust_final.summary.rust_current_index_generation;
    }
    &metrics
        .rust_population
        .summary
        .rust_current_index_generation
}

pub(super) fn rust_cache_pruned_entries(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_cache_pruned_entries
        + metrics.rust_final.summary.rust_cache_pruned_entries
}

pub(super) fn rust_process_residual_count(metrics: &LocalRubricMetrics) -> usize {
    metrics.rust_population.summary.rust_process_residual_count
        + metrics.rust_final.summary.rust_process_residual_count
}

pub(super) fn rust_legacy_cleanup_deferred(metrics: &LocalRubricMetrics) -> bool {
    metrics.rust_population.summary.rust_legacy_cleanup_deferred
        || metrics.rust_final.summary.rust_legacy_cleanup_deferred
}
