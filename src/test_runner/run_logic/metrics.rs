use std::fs;
use std::path::Path;
use std::time::Duration;

use super::runners::SelectorExecutionSummary;
use super::{PlannedSelectors, SelectorRunOptions};

#[derive(Default)]
pub(super) struct PhaseMetrics {
    pub(super) duration: Duration,
    pub(super) summary: SelectorExecutionSummary,
}

pub(super) struct LocalRubricMetrics {
    pub(super) plan_duration: Duration,
    pub(super) total_duration: Duration,
    pub(super) selected_python: usize,
    pub(super) python_population_required: bool,
    pub(super) python_population_selectors: usize,
    pub(super) selected_rust_initial: usize,
    pub(super) rust_source_paths: usize,
    pub(super) rust_population_source_paths: usize,
    pub(super) rust_population_required: bool,
    pub(super) rust_population_selectors: usize,
    pub(super) rust_final_selectors: usize,
    pub(super) coverage_decision_engine_used: bool,
    pub(super) python: PhaseMetrics,
    pub(super) python_index_rebuild_duration: Duration,
    pub(super) rust_population: PhaseMetrics,
    pub(super) rust_index_rebuild_duration: Duration,
    pub(super) rust_final: PhaseMetrics,
    pub(super) kiss_cache_residual_bytes: u64,
    pub(super) rust_cache_residual_bytes: u64,
    pub(super) raw_artifact_count: usize,
    pub(super) worker_slot_count: usize,
    pub(super) worker_slot_limit: usize,
    pub(super) exit_code: i32,
}

impl LocalRubricMetrics {
    pub(super) fn new(
        planned: &PlannedSelectors,
        options: &SelectorRunOptions<'_>,
        rust_population_required: bool,
        rust_population_selectors: usize,
        rust_final_selectors: usize,
    ) -> Self {
        Self {
            plan_duration: options.plan_duration,
            total_duration: Duration::ZERO,
            selected_python: planned.py_sel.len(),
            python_population_required: planned.python_population_required,
            python_population_selectors: planned.python_population_selectors.len(),
            selected_rust_initial: planned.rs_sel.len(),
            rust_source_paths: planned.rust_source_paths.len(),
            rust_population_source_paths: planned.rust_source_population_paths.len(),
            rust_population_required,
            rust_population_selectors,
            rust_final_selectors,
            coverage_decision_engine_used: planned.coverage_decision_engine_used,
            python: PhaseMetrics::default(),
            python_index_rebuild_duration: Duration::ZERO,
            rust_population: PhaseMetrics::default(),
            rust_index_rebuild_duration: Duration::ZERO,
            rust_final: PhaseMetrics::default(),
            kiss_cache_residual_bytes: 0,
            rust_cache_residual_bytes: 0,
            raw_artifact_count: 0,
            worker_slot_count: 0,
            worker_slot_limit: options.jobs,
            exit_code: 0,
        }
    }

    pub(super) fn capture_cache_shape(&mut self, repo_root: &Path) {
        let kiss_cache = repo_root.join(".kiss");
        let rust_cache = kiss_cache.join("rust_llvm_cov_cache");
        self.kiss_cache_residual_bytes = path_size_bytes(&kiss_cache);
        self.rust_cache_residual_bytes = path_size_bytes(&rust_cache);
        self.raw_artifact_count = count_json_files(&rust_cache.join("artifacts"));
        self.worker_slot_count = count_worker_slots(&rust_cache.join("workers"));
    }

    pub(super) fn print(&self) {
        print_oracle_metrics();
        print_selection_metrics(self);
        print_phase_metrics("python", &self.python);
        print_phase_metrics("rust_population", &self.rust_population);
        print_phase_metrics("rust_final", &self.rust_final);
        print_timing_metrics(self);
        print_cache_metrics(self);
        println!("exit_code={}", self.exit_code);
    }
}

fn print_oracle_metrics() {
    println!("KISS TEST METRICS");
    println!("oracle_selector_recall=external_required");
    println!("oracle_false_negative_rate=external_required");
    println!("oracle_exit_code_agreement=external_required");
    println!("selection_ratio=external_full_suite_count_required");
    println!("time_saved_ratio=external_full_suite_time_required");
}

fn print_selection_metrics(metrics: &LocalRubricMetrics) {
    println!("selected_python={}", metrics.selected_python);
    println!(
        "python_population_required={}",
        metrics.python_population_required
    );
    println!(
        "python_population_selectors={}",
        metrics.python_population_selectors
    );
    println!("selected_rust_initial={}", metrics.selected_rust_initial);
    println!("rust_source_paths={}", metrics.rust_source_paths);
    println!(
        "rust_population_source_paths={}",
        metrics.rust_population_source_paths
    );
    println!(
        "rust_population_required={}",
        metrics.rust_population_required
    );
    println!(
        "rust_population_selectors={}",
        metrics.rust_population_selectors
    );
    println!("rust_final_selectors={}", metrics.rust_final_selectors);
    println!(
        "coverage_decision_engine_used={}",
        metrics.coverage_decision_engine_used
    );
}

fn print_timing_metrics(metrics: &LocalRubricMetrics) {
    println!("phase_plan_ms={}", metrics.plan_duration.as_millis());
    println!("phase_python_ms={}", metrics.python.duration.as_millis());
    println!(
        "phase_python_index_rebuild_ms={}",
        metrics.python_index_rebuild_duration.as_millis()
    );
    println!(
        "phase_rust_population_ms={}",
        metrics.rust_population.duration.as_millis()
    );
    println!(
        "phase_rust_index_rebuild_ms={}",
        metrics.rust_index_rebuild_duration.as_millis()
    );
    println!(
        "phase_rust_final_ms={}",
        metrics.rust_final.duration.as_millis()
    );
    println!("phase_total_ms={}", metrics.total_duration.as_millis());
}

fn print_cache_metrics(metrics: &LocalRubricMetrics) {
    println!(
        "kiss_cache_residual_bytes={}",
        metrics.kiss_cache_residual_bytes
    );
    println!(
        "rust_cache_residual_bytes={}",
        metrics.rust_cache_residual_bytes
    );
    println!("raw_artifact_count={}", metrics.raw_artifact_count);
    println!("worker_slot_count={}", metrics.worker_slot_count);
    println!("worker_slot_limit={}", metrics.worker_slot_limit);
    println!(
        "raw_artifact_residuals_pass={}",
        metrics.raw_artifact_count == 0
    );
    println!(
        "worker_slot_bound_pass={}",
        metrics.worker_slot_count <= metrics.worker_slot_limit
    );
}

fn print_phase_metrics(name: &str, phase: &PhaseMetrics) {
    println!("{name}_total={}", phase.summary.total);
    println!("{name}_cache_hits={}", phase.summary.cache_hits);
    println!("{name}_cache_misses={}", phase.summary.cache_misses);
    println!("{name}_failed={}", phase.summary.failed);
}

fn path_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| path_size_bytes(&entry.path()))
        .sum()
}

fn count_json_files(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .count()
}

fn count_worker_slots(path: &Path) -> usize {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("slot-"))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_metrics_prints_zero_summary() {
        let phase = PhaseMetrics::default();

        assert_eq!(phase.summary.total, 0);
        print_phase_metrics("phase", &phase);
    }

    #[test]
    fn metrics_print_helpers_accept_empty_metrics() {
        let metrics = LocalRubricMetrics {
            plan_duration: Duration::ZERO,
            total_duration: Duration::ZERO,
            selected_python: 0,
            python_population_required: false,
            python_population_selectors: 0,
            selected_rust_initial: 0,
            rust_source_paths: 0,
            rust_population_source_paths: 0,
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
            raw_artifact_count: 0,
            worker_slot_count: 0,
            worker_slot_limit: 1,
            exit_code: 0,
        };

        print_oracle_metrics();
        print_selection_metrics(&metrics);
        print_timing_metrics(&metrics);
        print_cache_metrics(&metrics);
        metrics.print();
    }
}
